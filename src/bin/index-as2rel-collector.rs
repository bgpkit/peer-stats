use chrono::{NaiveDate, Utc};
use clap::Parser;
use peer_stats::As2Rel;
use std::collections::HashMap;
use std::io::{BufWriter, Read, Write};
use std::path::PathBuf;
use tracing::info;
use walkdir::WalkDir;

/// Index AS relationship data with per-collector provenance.
/// Uses flat vector + sort + group to stay memory-efficient.
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

    // Flat vector of records — much more memory-efficient than nested HashMaps
    let mut records: Vec<FlatRecord> = Vec::new();

    for file in &file_paths {
        info!("processing {}", file.as_str());
        let mut data = String::new();
        oneio::get_reader(file.as_str())
            .unwrap()
            .read_to_string(&mut data)
            .unwrap();
        // Drop the raw string memory as soon as parsing is done
        let as2rel_info: As2Rel = {
            let result = serde_json::from_str(&data);
            drop(data);
            result.unwrap()
        };

        let project = &as2rel_info.project;
        let collector = &as2rel_info.collector;

        // Get or assign collector index
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
        "collected {} flat records from {} files, {} unique collectors",
        records.len(),
        input_file_count,
        collector_info.len()
    );

    // Sort by (asn1, asn2, rel, collector_idx)
    records.sort_unstable_by_key(|r| (r.0, r.1, r.2, r.3));

    // Group and write JSON manually (streaming — no intermediate Value tree)
    let output_file = format!(
        "{}/{}-collector-latest.json.bz2",
        opts.output_dir.to_str().unwrap(),
        file_prefix.strip_suffix('_').unwrap()
    );
    let file = std::fs::File::create(&output_file).unwrap();
    let compressor = bzip2::write::BzEncoder::new(file, bzip2::Compression::best());
    let mut writer = BufWriter::with_capacity(256 * 1024, compressor);

    let generated_at = Utc::now().to_rfc3339();

    // Write JSON header
    write!(
        writer,
        "{{\"generated_at\":{},\"input_files\":{},\"entries\":[",
        serde_json::to_string(&generated_at).unwrap(),
        input_file_count
    )
    .unwrap();

    let mut first_group = true;
    let mut i = 0;
    while i < records.len() {
        let key = (records[i].0, records[i].1, records[i].2);

        // Find all records for this key
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

        // Write comma separator
        if !first_group {
            write!(writer, ",").unwrap();
        }
        first_group = false;

        // Write entry header
        write!(
            writer,
            "{{\"asn1\":{},\"asn2\":{},\"rel\":{},\"total_paths_count\":{},\"total_peers_count\":{},\"collector_count\":{},\"collectors\":{{",
            key.0, key.1, key.2, total_paths, total_peers, collector_count
        )
        .unwrap();

        // Write per-collector breakdown
        for (j, rec) in group_slice.iter().enumerate() {
            let info = &collector_info[rec.3 as usize];
            if j > 0 {
                write!(writer, ",").unwrap();
            }
            write!(
                writer,
                "{}:{{\"project\":{},\"collector\":{},\"paths_count\":{},\"peers_count\":{}}}",
                serde_json::to_string(&info.name).unwrap(),
                serde_json::to_string(&info.project).unwrap(),
                serde_json::to_string(&info.name).unwrap(),
                rec.4,
                rec.5
            )
            .unwrap();
        }

        write!(writer, "}}}}").unwrap();
    }

    write!(writer, "]}}").unwrap();
    writer.flush().unwrap();

    info!("wrote output to {}", output_file);
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
