use clap::Parser;
use peer_stats::parse_rib_file;
use serde_json::json;
use std::path::PathBuf;
use tracing::{info, Level};

/// peer-stats is a CLI tool that collects peer information from a given RIB dump file.
#[derive(Parser, Debug)]
struct Opts {
    /// File path to a MRT file, local or remote.
    rib_file: PathBuf,

    /// whether to print debug
    #[clap(long)]
    debug: bool,
}

fn main() {
    let opts: Opts = Opts::parse();

    if opts.debug {
        tracing_subscriber::fmt()
            // filter spans/events with level TRACE or higher.
            .with_max_level(Level::INFO)
            .with_writer(std::io::stderr)
            .init();
    }

    let file_path = opts.rib_file.to_str().unwrap();
    info!("start parsing file {}", file_path);

    let mut project = "unknown".to_string();
    let mut collector = "unknown".to_string();
    if file_path.contains("routeviews") {
        project = "route-views".to_string();
        if file_path.contains("http") {
            let parts: Vec<&str> = file_path.split('/').collect::<Vec<&str>>();
            collector = parts[3].to_string();
        }
    } else if file_path.contains("rrc") {
        project = "riperis".to_string();
        if file_path.contains("http") {
            let parts: Vec<&str> = file_path.split('/').collect::<Vec<&str>>();
            collector = parts[3].to_string();
        }
    };

    let (peer_stats, _pfx2as, _as2rel) =
        parse_rib_file(file_path, project.as_str(), collector.as_str()).unwrap();

    println!(
        "{}",
        serde_json::to_string_pretty(&json!(peer_stats)).unwrap()
    );
    info!("finished");
}
