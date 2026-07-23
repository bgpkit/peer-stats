use chrono::{NaiveDate, Utc};
use clap::Parser;
use peer_stats::Prefix2As;
use serde::Serialize;
use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use tracing::info;
use walkdir::WalkDir;

/// Index prefix-to-AS mapping data with per-collector provenance.
/// Produces pfx2as-collector-latest.json.bz2 with collector-level breakdown.
#[derive(Parser, Debug)]
struct Opts {
    /// Path to output directory (file named pfx2as-collector-latest.json.bz2)
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
struct Pfx2AsCollectorDetail {
    project: String,
    collector: String,
    count: usize,
}

#[derive(Debug, Clone, Serialize)]
struct Pfx2AsCollectorEntry {
    prefix: String,
    asn: u32,
    /// Total count across all collectors
    total_count: usize,
    /// Number of distinct collectors seeing this mapping
    collector_count: usize,
    /// Per-collector breakdown
    collectors: HashMap<String, Pfx2AsCollectorDetail>,
}

#[derive(Debug, Clone, Serialize)]
struct Pfx2AsCollectorOutput {
    generated_at: String,
    input_files: usize,
    entries: Vec<Pfx2AsCollectorEntry>,
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
        info!("no data files found, skipping");
        return;
    }

    let input_file_count = file_paths.len();

    // Key: (prefix, asn) — Value: per-collector details
    let mut collector_map: HashMap<(String, u32), HashMap<String, Pfx2AsCollectorDetail>> =
        HashMap::new();

    for file in &file_paths {
        info!("processing {}", file.as_str());
        let mut data = String::new();
        oneio::get_reader(file.as_str())
            .unwrap()
            .read_to_string(&mut data)
            .unwrap();
        let pfx2as_info: Prefix2As = serde_json::from_str(&data).unwrap();

        let project = pfx2as_info.project;
        let collector = pfx2as_info.collector;

        for pfx2as in pfx2as_info.pfx2as {
            let key = (pfx2as.prefix.clone(), pfx2as.asn);
            let per_collector = collector_map.entry(key).or_default();
            let detail =
                per_collector
                    .entry(collector.clone())
                    .or_insert_with(|| Pfx2AsCollectorDetail {
                        project: project.clone(),
                        collector: collector.clone(),
                        count: 0,
                    });
            detail.count += pfx2as.count;
        }
    }

    let entry_count = collector_map.len();
    let entries: Vec<Pfx2AsCollectorEntry> = collector_map
        .into_iter()
        .map(|((prefix, asn), per_collector)| {
            let total_count: usize = per_collector.values().map(|d| d.count).sum();
            let collector_count = per_collector.len();

            Pfx2AsCollectorEntry {
                prefix,
                asn,
                total_count,
                collector_count,
                collectors: per_collector,
            }
        })
        .collect();

    let output = Pfx2AsCollectorOutput {
        generated_at: Utc::now().to_rfc3339(),
        input_files: input_file_count,
        entries,
    };

    let output_file = format!(
        "{}/pfx2as-collector-latest.json.bz2",
        opts.output_dir.to_str().unwrap()
    );
    let mut writer = oneio::get_writer(output_file.as_str()).unwrap();
    let _ = writer.write_all(
        serde_json::to_string_pretty(&serde_json::to_value(&output).unwrap())
            .unwrap()
            .as_ref(),
    );
    info!("wrote {} entries to {}", entry_count, output_file);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_file_date() {
        assert_eq!(
            get_ymd_from_file("pfx2as_rrc16_2022-02-01_1643673600.bz2"),
            (2022, 2, 1)
        );
        assert_eq!(
            get_ymd_from_file("/aaa_bbb-ccc/pfx2as_rrc16_2022-02-01_1643673600.bz2"),
            (2022, 2, 1)
        );
    }
}
