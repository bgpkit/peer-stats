use chrono::{Datelike, NaiveDate, Utc};
use clap::Parser;
use peer_stats::{Prefix2As, Prefix2AsCount};
use serde_json::json;
use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use tracing::info;
use walkdir::WalkDir;

/// peer-stats is a CLI tool that collects peer information from a given RIB dump file.
#[derive(Parser, Debug)]
struct Opts {
    /// Path to output file
    output_file: PathBuf,

    /// Path to the data file directory
    data_dir: PathBuf,

    /// whether to print debug
    #[clap(long)]
    debug: bool,

    #[clap(long)]
    allow_previous_day: bool,
}

fn get_ymd_from_file(file_path: &str) -> (i32, u32, u32) {
    let date_part = file_path.split('_').collect::<Vec<&str>>();
    let parts = date_part[date_part.len() - 2]
        .split('-')
        .collect::<Vec<&str>>();
    (
        parts[0].parse::<i32>().unwrap(),
        parts[1].parse::<u32>().unwrap(),
        parts[2].parse::<u32>().unwrap(),
    )
}

fn main() {
    let opts = Opts::parse();

    if opts.debug {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::INFO)
            .init();
    }

    let file_paths = WalkDir::new(opts.data_dir.to_str().unwrap())
        .follow_links(true)
        .into_iter()
        .filter_map(|e| match e.ok() {
            Some(entry) => {
                let path: String = entry.path().to_str().unwrap().to_string();
                let path_str = path.as_str();
                if path_str.contains("pfx2as_") && path_str.ends_with(".bz2") {
                    let (year, month, day) = get_ymd_from_file(path.as_str());
                    let file_date = NaiveDate::from_ymd_opt(year, month, day).unwrap();
                    let ts = Utc::now().date().naive_utc();
                    if file_date == ts {
                        return Some(path);
                    }
                    if opts.allow_previous_day && file_date == ts.pred() {
                        return Some(path);
                    }
                }
                None
            }
            None => None,
        })
        .collect::<Vec<String>>();

    if file_paths.is_empty() {
        info!("no data files found, skipping");
        return;
    }

    let mut data_map: HashMap<(String, u32), usize> = HashMap::new();

    for file in file_paths {
        info!("processing {}", file.as_str());
        let mut data = "".to_string();
        oneio::get_reader(file.as_str())
            .unwrap()
            .read_to_string(&mut data)
            .unwrap();
        let pfx2as_info: Prefix2As = serde_json::from_str(&data).unwrap();

        for pfx2as in pfx2as_info.pfx2as {
            let (prefix, asn, count) = (pfx2as.prefix, pfx2as.asn, pfx2as.count);
            let total_count = data_map.entry((prefix, asn)).or_insert(0);
            *total_count += count;
        }
    }

    let res: Vec<Prefix2AsCount> = data_map
        .into_iter()
        .map(|((prefix, asn), count)| Prefix2AsCount { prefix, asn, count })
        .collect();

    let mut writer = oneio::get_writer(opts.output_file.to_str().unwrap()).unwrap();
    let _ = writer.write_all(serde_json::to_string_pretty(&json!(res)).unwrap().as_ref());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_file_date() {
        assert_eq!(
            get_ymd_from_file("as2rel_rrc16_2022-02-01_1643673600.bz2"),
            (2022, 2, 1)
        );
        assert_eq!(
            get_ymd_from_file("/aaa_bbb-ccc/as2rel_rrc16_2022-02-01_1643673600.bz2"),
            (2022, 2, 1)
        );
    }
}
