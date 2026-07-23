use chrono::{NaiveDate, Utc};
use clap::Parser;
use peer_stats::Prefix2As;
use std::collections::HashMap;
use std::io::{BufWriter, Read, Write};
use std::path::PathBuf;
use tracing::info;
use walkdir::WalkDir;

/// Index prefix-to-AS mapping data with per-collector provenance.
/// Uses flat vector + sort + group to stay memory-efficient.
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

/// Flat record: (prefix, asn, collector_idx, count)
/// We use a string interning approach: store prefix strings in a vec and reference by index.
type FlatRecord = (u32, u32, u16, usize); // (prefix_idx, asn, collector_idx, count)

struct CollectorInfo {
    name: String,
    project: String,
}

fn main() {
    let opts = Opts::parse();

    if opts.debug {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::INFO)
            .init();
    }

    let file_paths: Vec<String> = WalkDir::new(opts.data_dir.to_str().unwrap())
        .follow_links(true)
        .into_iter()
        .filter_map(|e| match e.ok() {
            Some(entry) => {
                let path: String = entry.path().to_str().unwrap().to_string();
                if path.contains("pfx2as_") && path.ends_with(".bz2") {
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
        info!("no data files found, skipping");
        return;
    }

    let input_file_count = file_paths.len();

    // Collector name → compact index
    let mut collector_index: HashMap<String, u16> = HashMap::new();
    let mut collector_info: Vec<CollectorInfo> = Vec::new();

    // Prefix string → compact index (interning)
    let mut prefix_index: HashMap<String, u32> = HashMap::new();
    let mut prefix_strings: Vec<String> = Vec::new();

    // Flat vector of records
    let mut records: Vec<FlatRecord> = Vec::new();

    for file in &file_paths {
        info!("processing {}", file.as_str());
        let mut data = String::new();
        oneio::get_reader(file.as_str())
            .unwrap()
            .read_to_string(&mut data)
            .unwrap();
        let pfx2as_info: Prefix2As = {
            let result = serde_json::from_str(&data);
            drop(data);
            result.unwrap()
        };

        let project = &pfx2as_info.project;
        let collector = &pfx2as_info.collector;

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

        for pfx2as in &pfx2as_info.pfx2as {
            let pidx = if let Some(&idx) = prefix_index.get(&pfx2as.prefix) {
                idx
            } else {
                let idx = prefix_strings.len() as u32;
                prefix_index.insert(pfx2as.prefix.clone(), idx);
                prefix_strings.push(pfx2as.prefix.clone());
                idx
            };

            records.push((pidx, pfx2as.asn, cidx, pfx2as.count));
        }
    }

    info!(
        "collected {} flat records from {} files, {} unique collectors, {} unique prefixes",
        records.len(),
        input_file_count,
        collector_info.len(),
        prefix_strings.len()
    );

    // Sort by (prefix_idx, asn, collector_idx)
    records.sort_unstable_by_key(|r| (r.0, r.1, r.2));

    // Group and write JSON manually
    let output_file = format!(
        "{}/pfx2as-collector-latest.json.bz2",
        opts.output_dir.to_str().unwrap()
    );
    let file = std::fs::File::create(&output_file).unwrap();
    let compressor = bzip2::write::BzEncoder::new(file, bzip2::Compression::best());
    let mut writer = BufWriter::with_capacity(256 * 1024, compressor);

    let generated_at = Utc::now().to_rfc3339();

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
        let key = (records[i].0, records[i].1); // (prefix_idx, asn)

        let group_start = i;
        let mut total_count = 0usize;
        while i < records.len() && records[i].0 == key.0 && records[i].1 == key.1 {
            total_count += records[i].3;
            i += 1;
        }
        let group_slice = &records[group_start..i];
        let collector_count = group_slice.len();

        if !first_group {
            write!(writer, ",").unwrap();
        }
        first_group = false;

        let prefix_str = &prefix_strings[key.0 as usize];

        write!(
            writer,
            "{{\"prefix\":{},\"asn\":{},\"total_count\":{},\"collector_count\":{},\"collectors\":{{",
            serde_json::to_string(prefix_str).unwrap(),
            key.1,
            total_count,
            collector_count
        )
        .unwrap();

        for (j, rec) in group_slice.iter().enumerate() {
            let info = &collector_info[rec.2 as usize];
            if j > 0 {
                write!(writer, ",").unwrap();
            }
            write!(
                writer,
                "{}:{{\"project\":{},\"collector\":{},\"count\":{}}}",
                serde_json::to_string(&info.name).unwrap(),
                serde_json::to_string(&info.project).unwrap(),
                serde_json::to_string(&info.name).unwrap(),
                rec.3
            )
            .unwrap();
        }

        write!(writer, "}}}}").unwrap();
    }

    write!(writer, "]}}").unwrap();
    writer.flush().unwrap();

    info!("wrote output to {}", output_file);
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
