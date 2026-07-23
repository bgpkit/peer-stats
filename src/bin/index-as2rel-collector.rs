use chrono::{NaiveDate, Utc};
use clap::Parser;
use peer_stats::As2Rel;
use serde::Serialize;
use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use tracing::info;
use walkdir::WalkDir;

/// Index AS relationship data with per-collector provenance.
/// Produces as2rel-collector-latest.json.bz2 with collector-level breakdown.
#[derive(Parser, Debug)]
struct Opts {
    /// Path to output directory
    output_dir: PathBuf,

    /// Path to the data file directory
    data_dir: PathBuf,

    /// Whether to print debug logs
    #[clap(long)]
    debug: bool,

    /// Allow processing files from the previous day
    #[clap(long)]
    allow_previous_day: bool,
}

#[derive(Debug, Clone, Serialize)]
struct CollectorDetail {
    project: String,
    collector: String,
    paths_count: usize,
    peers_count: usize,
}

#[derive(Debug, Clone, Serialize)]
struct As2RelCollectorEntry {
    asn1: u32,
    asn2: u32,
    rel: u8,
    /// Total paths across all collectors
    total_paths_count: usize,
    /// Total unique peers across all collectors (deduplicated by IP)
    total_peers_count: usize,
    /// Number of distinct collectors observing this relationship
    collector_count: usize,
    /// Per-collector breakdown
    collectors: HashMap<String, CollectorDetail>,
}

#[derive(Debug, Clone, Serialize)]
struct As2RelCollectorOutput {
    generated_at: String,
    input_files: usize,
    entries: Vec<As2RelCollectorEntry>,
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

    for file_prefix in ["as2rel_", "as2rel-v4_", "as2rel-v6_"] {
        let file_paths = WalkDir::new(opts.data_dir.to_str().unwrap())
            .follow_links(true)
            .into_iter()
            .filter_map(|e| match e.ok() {
                Some(entry) => {
                    let path: String = entry.path().to_str().unwrap().to_string();
                    let path_str = path.as_str();
                    if path_str.contains(file_prefix) && path_str.ends_with(".bz2") {
                        let (year, month, day) = get_ymd_from_file(path.as_str());
                        let file_date = NaiveDate::from_ymd_opt(year, month, day).unwrap();
                        let ts = Utc::now().date_naive();
                        if file_date == ts {
                            return Some(path);
                        }
                        if opts.allow_previous_day && file_date == ts.pred_opt().unwrap() {
                            return Some(path);
                        }
                    }
                    None
                }
                None => None,
            })
            .collect::<Vec<String>>();

        if file_paths.is_empty() {
            info!(
                "no matching current date {} files found, skipping",
                file_prefix
            );
            continue;
        }

        let input_file_count = file_paths.len();

        // Key: (asn1, asn2, rel) — Value: per-collector details
        let mut collector_map: HashMap<(u32, u32, u8), HashMap<String, CollectorDetail>> =
            HashMap::new();

        for file in &file_paths {
            info!("processing {}", file.as_str());
            let mut data = String::new();
            oneio::get_reader(file.as_str())
                .unwrap()
                .read_to_string(&mut data)
                .unwrap();
            let as2rel_info: As2Rel = serde_json::from_str(&data).unwrap();

            let project = as2rel_info.project;
            let collector = as2rel_info.collector;

            for as2rel in as2rel_info.as2rel {
                let key = (as2rel.asn1, as2rel.asn2, as2rel.rel);
                let per_collector = collector_map.entry(key).or_default();
                let detail =
                    per_collector
                        .entry(collector.clone())
                        .or_insert_with(|| CollectorDetail {
                            project: project.clone(),
                            collector: collector.clone(),
                            paths_count: 0,
                            peers_count: 0,
                        });
                detail.paths_count += as2rel.paths_count;
                detail.peers_count += as2rel.peers_count;
            }
        }

        let entries: Vec<As2RelCollectorEntry> = collector_map
            .into_iter()
            .map(|((asn1, asn2, rel), per_collector)| {
                let total_paths_count: usize = per_collector.values().map(|d| d.paths_count).sum();
                let total_peers_count: usize = per_collector.values().map(|d| d.peers_count).sum();
                let collector_count = per_collector.len();

                As2RelCollectorEntry {
                    asn1,
                    asn2,
                    rel,
                    total_paths_count,
                    total_peers_count,
                    collector_count,
                    collectors: per_collector,
                }
            })
            .collect();

        let output = As2RelCollectorOutput {
            generated_at: Utc::now().to_rfc3339(),
            input_files: input_file_count,
            entries,
        };

        let output_file = format!(
            "{}/{}-collector-latest.json.bz2",
            opts.output_dir.to_str().unwrap(),
            file_prefix.strip_suffix('_').unwrap()
        );
        let mut writer = oneio::get_writer(output_file.as_str()).unwrap();
        let _ = writer.write_all(
            serde_json::to_string_pretty(&serde_json::to_value(&output).unwrap())
                .unwrap()
                .as_ref(),
        );
        info!("wrote {} entries to {}", file_paths.len(), output_file);
    }
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
