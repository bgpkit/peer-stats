use chrono::{NaiveDate, Utc};
use clap::Parser;
use peer_stats::As2Rel;
use std::collections::HashMap;
use std::io::{BufWriter, Read, Write};
use std::path::PathBuf;
use tracing::info;
use walkdir::WalkDir;

/// Index AS relationship data from per-collector daily files.
///
/// Produces TWO output files per prefix (as2rel, as2rel-v4, as2rel-v6):
///   1. Classic: {prefix}-latest.json.bz2 — unchanged v1 aggregate
///   2. Collector: {prefix}-collector-latest.json.bz2 — per-collector provenance
///
/// Both are generated from a single pass over the daily files.
/// The classic output format is identical to the previous indexer — no breaking changes.
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

/// Flat record: (asn1, asn2, rel, collector_idx, paths_count, peers_count)
type FlatRecord = (u32, u32, u8, u16, usize, usize);

struct CollectorInfo {
    name: String,
    project: String,
}

fn process_prefix(file_prefix: &str, opts: &Opts) {
    let file_paths: Vec<String> = WalkDir::new(opts.data_dir.to_str().unwrap())
        .follow_links(true)
        .into_iter()
        .filter_map(|e| match e.ok() {
            Some(entry) => {
                let path: String = entry.path().to_str().unwrap().to_string();
                if path.contains(file_prefix) && path.ends_with(".bz2") {
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
        .collect();

    if file_paths.is_empty() {
        info!(
            "no matching current date {} files found, skipping",
            file_prefix
        );
        return;
    }

    let input_file_count = file_paths.len();

    // Collector name → compact index
    let mut collector_index: HashMap<String, u16> = HashMap::new();
    let mut collector_info: Vec<CollectorInfo> = Vec::new();

    // --- Phase 1: collect flat records ---
    let mut records: Vec<FlatRecord> = Vec::new();

    for file in &file_paths {
        info!("reading {}", file.as_str());
        let mut data = String::new();
        oneio::get_reader(file.as_str())
            .unwrap()
            .read_to_string(&mut data)
            .unwrap();
        let as2rel_info: As2Rel = {
            let result = serde_json::from_str(&data);
            drop(data);
            result.unwrap()
        };

        let collector = &as2rel_info.collector;
        let project = &as2rel_info.project;

        let cidx = if let Some(&idx) = collector_index.get(collector) {
            idx
        } else {
            let idx = collector_info.len() as u16;
            collector_index.insert(collector.clone(), idx);
            collector_info.push(CollectorInfo {
                name: collector.clone(),
                project: project.clone(),
            });
            idx
        };

        for as2rel in &as2rel_info.as2rel {
            records.push((
                as2rel.asn1,
                as2rel.asn2,
                as2rel.rel,
                cidx,
                as2rel.paths_count,
                as2rel.peers_count,
            ));
        }
    }

    info!(
        "{}: collected {} records from {} files, {} collectors",
        file_prefix,
        records.len(),
        input_file_count,
        collector_info.len()
    );

    // --- Phase 2: sort by key + collector ---
    records.sort_unstable_by_key(|r| (r.0, r.1, r.2, r.3));

    // --- Phase 3: group and write both outputs ---
    let base_name = file_prefix.strip_suffix('_').unwrap();

    // Classic output
    let classic_file = format!(
        "{}/{}-latest.json.bz2",
        opts.output_dir.to_str().unwrap(),
        base_name
    );
    let classic_raw = std::fs::File::create(&classic_file).unwrap();
    let classic_comp = bzip2::write::BzEncoder::new(classic_raw, bzip2::Compression::best());
    let mut classic_w = BufWriter::with_capacity(256 * 1024, classic_comp);

    // Collector output
    let collector_file = format!(
        "{}/{}-collector-latest.json.bz2",
        opts.output_dir.to_str().unwrap(),
        base_name
    );
    let collector_raw = std::fs::File::create(&collector_file).unwrap();
    let collector_comp = bzip2::write::BzEncoder::new(collector_raw, bzip2::Compression::best());
    let mut collector_w = BufWriter::with_capacity(256 * 1024, collector_comp);

    let generated_at = Utc::now().to_rfc3339();

    // Classic: opening bracket
    write!(classic_w, "[").unwrap();

    // Collector: header
    write!(
        collector_w,
        "{{\"generated_at\":{},\"input_files\":{},\"entries\":[",
        serde_json::to_string(&generated_at).unwrap(),
        input_file_count
    )
    .unwrap();

    let mut first_classic = true;
    let mut first_collector = true;
    let mut i = 0;
    let mut entry_count: usize = 0;

    while i < records.len() {
        let key = (records[i].0, records[i].1, records[i].2);

        // Find all records for this (asn1, asn2, rel) key
        let group_start = i;
        let mut total_paths = 0usize;
        let mut total_peers = 0usize;
        while i < records.len()
            && records[i].0 == key.0
            && records[i].1 == key.1
            && records[i].2 == key.2
        {
            total_paths += records[i].4;
            total_peers += records[i].5;
            i += 1;
        }
        let group_slice = &records[group_start..i];
        let collector_count = group_slice.len();
        entry_count += 1;

        // --- Classic output: single As2RelCount ---
        if !first_classic {
            write!(classic_w, ",").unwrap();
        }
        first_classic = false;
        write!(
            classic_w,
            "{{\"asn1\":{},\"asn2\":{},\"rel\":{},\"paths_count\":{},\"peers_count\":{}}}",
            key.0, key.1, key.2, total_paths, total_peers
        )
        .unwrap();

        // --- Collector output: full entry with collectors map ---
        if !first_collector {
            write!(collector_w, ",").unwrap();
        }
        first_collector = false;
        write!(
            collector_w,
            "{{\"asn1\":{},\"asn2\":{},\"rel\":{},\"total_paths_count\":{},\"total_peers_count\":{},\"collector_count\":{},\"collectors\":{{",
            key.0, key.1, key.2, total_paths, total_peers, collector_count
        )
        .unwrap();

        for (j, rec) in group_slice.iter().enumerate() {
            let info = &collector_info[rec.3 as usize];
            if j > 0 {
                write!(collector_w, ",").unwrap();
            }
            write!(
                collector_w,
                "{}:{{\"project\":{},\"collector\":{},\"paths_count\":{},\"peers_count\":{}}}",
                serde_json::to_string(&info.name).unwrap(),
                serde_json::to_string(&info.project).unwrap(),
                serde_json::to_string(&info.name).unwrap(),
                rec.4,
                rec.5
            )
            .unwrap();
        }

        write!(collector_w, "}}}}").unwrap();
    }

    // Classic: closing bracket
    write!(classic_w, "]").unwrap();
    classic_w.flush().unwrap();
    drop(classic_w);

    // Collector: closing brackets
    write!(collector_w, "]}}").unwrap();
    collector_w.flush().unwrap();
    drop(collector_w);

    info!(
        "{}: wrote {} entries, classic={} collector={}",
        file_prefix, entry_count, classic_file, collector_file
    );
}

fn main() {
    let opts = Opts::parse();

    if opts.debug {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::INFO)
            .init();
    }

    for file_prefix in ["as2rel_", "as2rel-v4_", "as2rel-v6_"] {
        process_prefix(file_prefix, &opts);
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
