use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::net::IpAddr;
use std::path::PathBuf;
use bzip2::read::BzDecoder;
use chrono::{Datelike, Utc};
use rusqlite::Connection;
use serde::{Serialize, Deserialize};
use tracing::info;
use walkdir::{WalkDir};
use clap::Parser;

pub struct PeerStatsDb {
    db: Connection,
}

fn get_date_from_url(url: &str) -> (String, String, String) {
    let parts = url.split(".").collect::<Vec<&str>>();
    let date_str = parts[parts.len()-3];
    let year = date_str.get(0..=3).unwrap().to_string();
    let month = date_str.get(4..=5).unwrap().to_string();
    let day = date_str.get(6..=7).unwrap().to_string();
    return (year, month, day)
}

impl PeerStatsDb {
    pub fn new(db_path: &Option<String>) -> PeerStatsDb {
        let db = match db_path {
            Some(p) => {
                Connection::open(p.as_str()).unwrap()
            }
            None => {
                Connection::open_in_memory().unwrap()
            }
        };

        db.execute(r#"
        create table if not exists peer_stats (
        date TEXT ,
        collector TEXT,
        ip TEXT,
        asn INTEGER,
        num_v4_pfxs INTEGER,
        num_v6_pfxs INTEGER,
        num_connected_asns INTEGER,
        PRIMARY KEY (date, collector, ip)
        );
        "#, []).unwrap();

        db.execute(r#"
        create index if not exists date_index on peer_stats (
        date DESC
        );
        "#, []).unwrap();

        PeerStatsDb { db }
    }

    pub fn is_db_empty(&self) -> bool {
        let count: u32 = self.db.query_row("select count(*) from peer_stats", [],
                                           |row| row.get(0),
        ).unwrap();
        count == 0
    }

    pub fn insert_rib_info(&self, rib_info: &RibPeerInfo) -> bool {
        let (year, month, day) = get_date_from_url(rib_info.rib_dump_url.as_str());
        let date = format!("{}-{}-{}", year, month, day);
        for (ip, peer) in &rib_info.peers {
            let res = self.db.execute( r#"
        INSERT INTO peer_stats (date, collector, ip, asn, num_v4_pfxs, num_v6_pfxs, num_connected_asns)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        "#, (
                date.as_str(),
                rib_info.collector.as_str(),
                ip.to_string().as_str(),
                peer.asn,
                peer.num_v4_pfxs,
                peer.num_v6_pfxs,
                peer.num_connected_asns,
            )
            );
            if !res.is_ok() {
                return false
            }
        }
        true
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RibPeerInfo {
    project: String,
    collector: String,
    rib_dump_url: String,
    peers: HashMap<IpAddr, PeerInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    ip: IpAddr,
    asn: u32,
    num_v4_pfxs: usize,
    num_v6_pfxs: usize,
    num_connected_asns: usize,
}

/// peer-stats is a CLI tool that collects peer information from a given RIB dump file.
#[derive(Parser, Debug)]
struct Opts {
    /// Path to a sqlite3 database file
    db_file: PathBuf,

    /// Path to the data file directory
    data_dir: PathBuf,

    /// Whether to bootstrap the whole database, otherwise, only process the latest
    #[clap(long, short)]
    bootstrap: bool,

    /// whether to print debug
    #[clap(long)]
    debug: bool,
}

fn main(){
    let opts = Opts::parse();

    if opts.debug {
        tracing_subscriber::fmt().with_max_level(tracing::Level::INFO).init();
    }

    let db = PeerStatsDb::new(&Some(opts.db_file.to_str().unwrap().to_string()));

    let file_paths = WalkDir::new(opts.data_dir.to_str().unwrap().to_string())
        .follow_links(true)
        .into_iter()
        .filter_map(|e| {
            match e.ok() {
                Some(entry) => {
                    let path: String = entry.path().to_str().unwrap().to_string();
                    if path.as_str().ends_with(".bz2") {
                        if opts.bootstrap {
                            return Some(path)
                        } else {
                            let parts = path.as_str().split("-").collect::<Vec<&str>>();
                            let (year, month, day) = (
                                parts[parts.len()-4].parse::<i32>().unwrap(),
                                parts[parts.len()-3].parse::<u32>().unwrap(),
                                parts[parts.len()-2].parse::<u32>().unwrap(),
                                );
                            let ts = Utc::now();
                            let ts2 = ts - chrono::Duration::days(1);

                            let expected_dates = match ts.month() == ts2.month() {
                                true => {
                                    vec![
                                        (ts.year(), ts.month(),ts.day())
                                    ]
                                }
                                false => {
                                    vec![
                                        (ts.year(), ts.month(),ts.day()),
                                        (ts2.year(), ts2.month(),ts2.day()),
                                    ]
                                }
                            };

                            return if expected_dates.into_iter().any(|(y, m, d)| {
                                y == year && m == month && d == day
                            }) {
                                Some(path)
                            } else {
                                None
                            }
                        }
                    }
                    return None
                }
                None => {None}
            }
        }
        ).collect::<Vec<String>>();

    for file in file_paths {
        info!("processing {}", file.as_str());
        let mut reader = BufReader::new(BzDecoder::new(File::open(file.as_str()).unwrap()));
        let mut data = "".to_string();
        reader.read_to_string(&mut data).unwrap();
        let rib_info: RibPeerInfo = serde_json::from_str(&data).unwrap();
        if !db.insert_rib_info(&rib_info) {
            info!("data already exists, skipping: {}", file.as_str());
        } else {
            info!("processing {} finished ", file.as_str());
        }
    }
}
