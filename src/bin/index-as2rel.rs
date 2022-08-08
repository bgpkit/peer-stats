use std::collections::HashMap;
use std::io::{Read};
use std::path::PathBuf;
use chrono::{Datelike, Utc};
use tracing::info;
use walkdir::{WalkDir};
use clap::Parser;
use serde_json::json;
use peer_stats::{As2Rel, As2RelCount};

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
}

fn get_ymd_from_file(file_path: &str) -> (i32, u32, u32) {
    let date_part = file_path.split('_').collect::<Vec<&str>>();
    let parts = date_part[date_part.len()-2].split('-').collect::<Vec<&str>>();
    (
        parts[0].parse::<i32>().unwrap(),
        parts[1].parse::<u32>().unwrap(),
        parts[2].parse::<u32>().unwrap(),
    )
}

fn main(){
    let opts = Opts::parse();

    if opts.debug {
        tracing_subscriber::fmt().with_max_level(tracing::Level::INFO).init();
    }

    let file_paths = WalkDir::new(opts.data_dir.to_str().unwrap())
        .follow_links(true)
        .into_iter()
        .filter_map(|e| {
            match e.ok() {
                Some(entry) => {
                    let path: String = entry.path().to_str().unwrap().to_string();
                    let path_str = path.as_str();
                    if path_str.contains("as2rel_") && path_str.ends_with(".bz2") {
                        let (year, month, day) = get_ymd_from_file(path.as_str());
                        let ts = Utc::now();
                        if ts.year()==year && ts.month() == month && ts.day() == day {
                            return Some(path)
                        }
                    }
                    None
                }
                None => {None}
            }
        }).collect::<Vec<String>>();

    let mut data_map: HashMap<(u32, u32, u8), (usize, usize)> = HashMap::new();

    for file in file_paths {
        info!("processing {}", file.as_str());
        let mut data = "".to_string();
        oneio::get_reader(file.as_str()).unwrap().read_to_string(&mut data).unwrap();
        let as2rel_info: As2Rel = serde_json::from_str(&data).unwrap();

        for as2rel in as2rel_info.as2rel {
            let (asn1, asn2, rel, paths_count, peers_count) = (as2rel.asn1, as2rel.asn2, as2rel.rel, as2rel.paths_count, as2rel.peers_count);
            let (count_1, count_2) = data_map.entry((asn1, asn2, rel)).or_insert((0,0));
            *count_1 += paths_count;
            *count_2 += peers_count;
        }
    }

    let res: Vec<As2RelCount> = data_map.into_iter().map(|((asn1, asn2, rel), (paths_count, peers_count))|{
        As2RelCount { asn1, asn2, rel, paths_count, peers_count}
    }).collect();

    let mut writer = oneio::get_writer(opts.output_file.to_str().unwrap()).unwrap();
    let _ = writer.write_all(serde_json::to_string_pretty(&json!(res)).unwrap().as_ref());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_file_date() {
        assert_eq!(get_ymd_from_file("as2rel_rrc16_2022-02-01_1643673600.bz2"), (2022,2,1));
        assert_eq!(get_ymd_from_file("/aaa_bbb-ccc/as2rel_rrc16_2022-02-01_1643673600.bz2"), (2022,2,1));
    }
}