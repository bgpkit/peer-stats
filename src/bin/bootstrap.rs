use bgpkit_broker::{BgpkitBroker, BrokerItem};
use bzip2::write::BzEncoder;
use bzip2::Compression;
use chrono::{Datelike, Timelike};
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use peer_stats::parse_rib_file;
use rayon::prelude::*;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::{fs, thread};
use tracing::{error, info, Level};

/// peer-stats is a CLI tool that collects peer information from a given RIB dump file.
#[derive(Parser, Debug)]
#[structopt(name = "peer-stats")]
struct Opts {
    /// whether to print debug
    #[clap(long)]
    debug: bool,

    /// whether to dry run the code
    #[clap(long)]
    dry_run: bool,

    /// force run and replace any existing data files
    #[clap(long)]
    force: bool,

    /// whether to do only daily parsing
    #[clap(long)]
    only_daily: bool,

    /// start timestamp
    #[clap(long)]
    ts_start: String,

    /// end timestamp
    #[clap(long)]
    ts_end: String,

    /// Output directory
    #[clap(long)]
    output_dir: PathBuf,
}

fn write_results(output_path: &str, data: &Value) {
    let file = match File::create(output_path) {
        Err(_why) => panic!("couldn't open {}", output_path),
        Ok(file) => file,
    };

    let compressor = BzEncoder::new(file, Compression::best());
    let mut writer = BufWriter::with_capacity(128 * 1024, compressor);

    let _ = writer.write_all(serde_json::to_string_pretty(data).unwrap().as_ref());
}

fn main() {
    let opts = Opts::parse();

    if opts.debug {
        tracing_subscriber::fmt()
            // filter spans/events with level TRACE or higher.
            .with_max_level(Level::INFO)
            .with_writer(std::io::stderr)
            .init();
    }

    let num_threads = if let Ok(v) = std::env::var("MAX_THREADS") {
        if let Ok(t) = v.parse::<usize>() {
            t
        } else {
            num_cpus::get()
        }
    } else {
        num_cpus::get()
    };

    rayon::ThreadPoolBuilder::new()
        .num_threads(num_threads)
        .build_global()
        .unwrap();

    info!("using maximum {} threads for processing.", num_threads);

    info!("start querying broker for available RIB dump files.");
    let mut broker = BgpkitBroker::new()
        .ts_start(opts.ts_start.as_str())
        .ts_end(opts.ts_end.as_str())
        .data_type("rib")
        .page_size(10000);
    if let Ok(url) = std::env::var("BROKER_URL") {
        broker = broker.broker_url(url.as_str());
    }

    let items: Vec<BrokerItem> = broker
        .query()
        .unwrap()
        .into_iter()
        .filter(|item| {
            if !opts.only_daily {
                return true;
            }
            // only process the first one per-day
            item.ts_start.hour() == 0
        })
        .collect();
    let total_items = items.len();

    if opts.dry_run {
        info!("total of {total_items} RIB dump files to process");
        info!("first RIB is {}", items.first().unwrap().url);
        info!("last RIB is {}", items.last().unwrap().url);
        return;
    }

    let (sender_pb, receiver_pb) = channel::<String>();

    // dedicated thread for showing progress of the parsing
    thread::spawn(move || {
        let sty = ProgressStyle::default_bar()
            .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {eta} {msg}")
            .progress_chars("##-");
        let pb = ProgressBar::new(total_items as u64);
        pb.set_style(sty);
        for msg in receiver_pb.iter() {
            pb.set_message(&msg);
            pb.inc(1);
        }
    });

    let output_dir = opts.output_dir.to_str().unwrap();

    let data_types = ["peer-stats", "pfx2as", "as2rel"];

    items.par_iter().for_each_with(sender_pb, |s1, item| {
        let ts = item.ts_start;
        let timestamp = ts.timestamp();

        let mut file_path_map: HashMap<String, String> = HashMap::new();
        for data_type in data_types {
            let file_dir = format!(
                "{}/{}/{}/{:02}/{:02}",
                output_dir,
                data_type,
                &item.collector_id,
                ts.year(),
                ts.month()
            );
            fs::create_dir_all(file_dir.as_str()).unwrap();
            let output_path = format!(
                "{}/{}_{}_{}-{:02}-{:02}_{}.bz2",
                &file_dir,
                data_type,
                &item.collector_id,
                ts.year(),
                ts.month(),
                ts.day(),
                &timestamp
            );
            if !opts.force && std::path::Path::new(output_path.as_str()).exists() {
                info!(
                    "result file {} already exists, skip processing",
                    output_path
                );
                let _ = s1.send(format!("{}-{}", item.collector_id.as_str(), timestamp));
                return;
            }
            file_path_map.insert(data_type.to_string(), output_path);
        }

        let project = match item.collector_id.starts_with("rrc") {
            true => "riperis".to_string(),
            false => "route-views".to_string(),
        };

        // parsing and writing out info, manually scoping to potentially avoid memory issue
        {
            info!("start parsing file {}", item.url.as_str());
            let (peer_stats, pfx2as, as2rel) = match parse_rib_file(
                item.url.as_str(),
                project.as_str(),
                item.collector_id.as_str(),
            ) {
                Ok(i) => i,
                Err(_) => {
                    error!("processing of file {} failed", item.url.as_str());
                    let _ = s1.send(format!("{}-{}", item.collector_id.as_str(), timestamp));
                    return;
                }
            };

            write_results(
                file_path_map.get("peer-stats").unwrap().as_str(),
                &json!(peer_stats),
            );
            write_results(
                file_path_map.get("pfx2as").unwrap().as_str(),
                &json!(pfx2as),
            );
            write_results(
                file_path_map.get("as2rel").unwrap().as_str(),
                &json!(as2rel),
            );
        }

        let _ = s1.send(format!("{}-{}", item.collector_id.as_str(), timestamp));
        info!("processing file {} finished", item.url.as_str());
    });
}
