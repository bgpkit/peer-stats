#![allow(dead_code)]
use anyhow::Result;
use bgpkit_parser::BgpkitParser;
use ipnet::{IpNet, Ipv4Net, Ipv6Net};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;

#[derive(Debug, Clone, Serialize)]
pub struct RibPeerInfo {
    pub project: String,
    pub collector: String,
    pub rib_dump_url: String,
    pub peers: HashMap<IpAddr, PeerInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PeerInfo {
    pub ip: IpAddr,
    pub asn: u32,
    pub num_v4_pfxs: usize,
    pub num_v6_pfxs: usize,
    pub num_connected_asns: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prefix2As {
    pub project: String,
    pub collector: String,
    pub rib_dump_url: String,
    /// prefix to as mapping: <prefix, <asn, count>>
    pub pfx2as: Vec<Prefix2AsCount>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prefix2AsCount {
    pub prefix: String,
    pub asn: u32,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct As2Rel {
    pub project: String,
    pub collector: String,
    pub rib_dump_url: String,
    /// prefix to as mapping: <prefix, <asn, count>>
    pub as2rel: Vec<As2RelCount>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct As2RelCount {
    pub asn1: u32,
    pub asn2: u32,
    /// 1 - asn1 is upstream of asn2, 2 - peer, 0 - unknown
    pub rel: u8,
    /// number of paths having this relationship
    pub paths_count: usize,
    /// number of peers seeing this relationship
    pub peers_count: usize,
}

const TIER1: [u32; 17] = [
    6762, 12956, 2914, 3356, 6453, 1239, 701, 6461, 3257, 1299, 3491, 7018, 3320, 5511, 6830, 174,
    6939,
];

fn dedup_path(path: Vec<u32>) -> Vec<u32> {
    if path.len() <= 1 {
        return path;
    }

    let mut new_path = vec![path[0]];

    for (asn1, asn2) in path.into_iter().tuple_windows::<(u32, u32)>() {
        if asn1 != asn2 {
            new_path.push(asn2)
        }
    }
    new_path
}

/// collect information from a provided RIB file
///
/// Info to collect:
/// - `project`
/// - `collector`
/// - `dump_url`
/// - `peer_ip`
/// - `peer_asn`
/// - `num_v4_pfxs`
/// - `num_v6_pfxs`
pub fn parse_rib_file(
    file_url: &str,
    project: &str,
    collector: &str,
) -> Result<(RibPeerInfo, Prefix2As, As2Rel)> {
    // peer-stats
    let mut peer_asn_map: HashMap<IpAddr, u32> = HashMap::new();
    let mut peer_connection: HashMap<IpAddr, HashSet<u32>> = HashMap::new();
    let mut peer_v4_pfxs_map: HashMap<IpAddr, HashSet<Ipv4Net>> = HashMap::new();
    let mut peer_v6_pfxs_map: HashMap<IpAddr, HashSet<Ipv6Net>> = HashMap::new();

    // pfx2as
    let mut pfx2as_map: HashMap<(String, u32), usize> = HashMap::new();

    // as2rel
    let mut as2rel_map: HashMap<(u32, u32, u8), (usize, HashSet<IpAddr>)> = HashMap::new();

    for (_elem_count, elem) in (BgpkitParser::new(file_url)?).into_iter().enumerate() {
        peer_asn_map
            .entry(elem.peer_ip)
            .or_insert(elem.peer_asn.asn);
        if let Some(as_path) = elem.as_path.clone() {
            if let Some(mut u32_path) = as_path.to_u32_vec() {
                u32_path.dedup();

                // peer-stats
                match u32_path.get(1) {
                    None => {}
                    Some(asn) => {
                        peer_connection
                            .entry(elem.peer_ip)
                            .or_default()
                            .insert(*asn);
                    }
                };

                // pfx2as
                match u32_path.last() {
                    None => {}
                    Some(asn) => {
                        let prefix = elem.prefix.to_string();
                        let count = pfx2as_map.entry((prefix, *asn)).or_insert(0);
                        *count += 1;
                    }
                }

                // counting peer relationships
                for (asn1, asn2) in u32_path.iter().tuple_windows::<(&u32, &u32)>() {
                    let (msg_count, peers) = as2rel_map
                        .entry((*asn1, *asn2, 0))
                        .or_insert((0, HashSet::new()));
                    *msg_count += 1;
                    peers.insert(elem.peer_ip);
                }

                let contains_tier1 = u32_path.iter().any(|x| TIER1.contains(x));

                u32_path.reverse();
                if contains_tier1 {
                    let mut first_tier1: usize = usize::MAX;
                    for (i, asn) in u32_path.iter().enumerate() {
                        if TIER1.contains(asn) && first_tier1 == usize::MAX {
                            first_tier1 = i;
                            break;
                        }
                    }

                    // origin to first tier 1
                    if first_tier1 < u32_path.len() - 1 {
                        for i in 0..first_tier1 {
                            let (asn1, asn2) =
                                (u32_path.get(i).unwrap(), u32_path.get(i + 1).unwrap());
                            let (msg_count, peers) = as2rel_map
                                .entry((*asn2, *asn1, 1))
                                .or_insert((0, HashSet::new()));
                            *msg_count += 1;
                            peers.insert(elem.peer_ip);
                        }
                    }
                }
            }
        }

        match elem.prefix.prefix {
            IpNet::V4(net) => {
                peer_v4_pfxs_map
                    .entry(elem.peer_ip)
                    .or_default()
                    .insert(net);
            }
            IpNet::V6(net) => {
                peer_v6_pfxs_map
                    .entry(elem.peer_ip)
                    .or_default()
                    .insert(net);
            }
        }
        drop(elem);
    }

    let mut peer_info_map: HashMap<IpAddr, PeerInfo> = HashMap::new();
    for (ip, asn) in peer_asn_map {
        let num_v4_pfxs = peer_v4_pfxs_map.entry(ip).or_default().len();
        let num_v6_pfxs = peer_v6_pfxs_map.entry(ip).or_default().len();
        let num_connected_asn = peer_connection.entry(ip).or_default().len();
        let ip_clone = ip;
        let asn_clone = asn;
        peer_info_map.insert(
            ip_clone,
            PeerInfo {
                ip: ip_clone,
                asn: asn_clone,
                num_v4_pfxs,
                num_v6_pfxs,
                num_connected_asns: num_connected_asn,
            },
        );
    }

    let pfx2as = pfx2as_map
        .into_iter()
        .map(|((prefix, asn), count)| Prefix2AsCount { prefix, asn, count })
        .collect();

    let as2rel = as2rel_map
        .into_iter()
        .map(|((asn1, asn2, rel), (msg_count, peers))| As2RelCount {
            asn1,
            asn2,
            rel,
            paths_count: msg_count,
            peers_count: peers.len(),
        })
        .collect();

    Ok((
        RibPeerInfo {
            project: project.to_string(),
            collector: collector.to_string(),
            rib_dump_url: file_url.to_string(),
            peers: peer_info_map,
        },
        Prefix2As {
            project: project.to_string(),
            collector: collector.to_string(),
            rib_dump_url: file_url.to_string(),
            pfx2as,
        },
        As2Rel {
            project: project.to_string(),
            collector: collector.to_string(),
            rib_dump_url: file_url.to_string(),
            as2rel,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs::File;
    use tracing::{info, Level};

    #[test]
    fn test_read_rib() {
        tracing_subscriber::fmt()
            // filter spans/events with level TRACE or higher.
            .with_max_level(Level::INFO)
            .init();
        info!("start");
        let (peer_stats, pfx2as, as2rel) = parse_rib_file("http://archive.routeviews.org/route-views.soxrs/bgpdata/2022.08/RIBS/rib.20220808.1400.bz2",
        "route-views", "route-views.sg").unwrap();
        serde_json::to_writer_pretty(
            &File::create("peer_info_example.json").unwrap(),
            &json!(peer_stats),
        )
        .unwrap();
        serde_json::to_writer_pretty(
            &File::create("pfx2as_example.json").unwrap(),
            &json!(pfx2as),
        )
        .unwrap();
        serde_json::to_writer_pretty(
            &File::create("as2rel_example.json").unwrap(),
            &json!(as2rel),
        )
        .unwrap();
        info!("finished");
    }

    #[test]
    fn test_dedup() {
        let empty: Vec<u32> = vec![];
        assert_eq!(dedup_path(empty.clone()), empty);
        assert_eq!(dedup_path(vec![1]), vec![1]);
        assert_eq!(dedup_path(vec![0, 1, 2, 3, 3, 3, 3]), vec![0, 1, 2, 3]);
        assert_eq!(
            dedup_path(vec![0, 1, 2, 3, 3, 3, 3, 4]),
            vec![0, 1, 2, 3, 4]
        );
        assert_eq!(
            dedup_path(vec![0, 1, 2, 3, 3, 3, 3, 4, 4, 4, 4]),
            vec![0, 1, 2, 3, 4]
        );
    }
}
