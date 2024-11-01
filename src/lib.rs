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

const TIER1_V4: [u32; 17] = [
    6762, 12956, 2914, 3356, 6453, 1239, 701, 6461, 3257, 1299, 3491, 7018, 3320, 5511, 6830, 174,
    0,
];

const TIER1_V6: [u32; 17] = [
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

fn update_as2rel_map(
    peer_ip: IpAddr,
    tier1: &[u32],
    data_map: &mut HashMap<(u32, u32, u8), (usize, HashSet<IpAddr>)>,
    // input AS path must be from collector ([0]) to origin ([last])
    original_as_path: &[u32],
) {
    let mut as_path = original_as_path.to_vec();

    // counting peer relationships
    for (asn1, asn2) in as_path.iter().tuple_windows::<(&u32, &u32)>() {
        let (msg_count, peers) = data_map
            .entry((*asn1, *asn2, 0))
            .or_insert((0, HashSet::new()));
        *msg_count += 1;
        peers.insert(peer_ip);
    }

    // counting provider-customer relationships
    as_path.reverse();
    let contains_tier1 = as_path.iter().any(|x| tier1.contains(x));
    if contains_tier1 {
        let mut first_tier1: usize = usize::MAX;
        for (i, asn) in as_path.iter().enumerate() {
            if tier1.contains(asn) && first_tier1 == usize::MAX {
                first_tier1 = i;
                break;
            }
        }

        // origin to first tier 1
        if first_tier1 < as_path.len() - 1 {
            for i in 0..first_tier1 {
                let (asn1, asn2) = (as_path.get(i).unwrap(), as_path.get(i + 1).unwrap());
                let (msg_count, peers) = data_map
                    .entry((*asn2, *asn1, 1))
                    .or_insert((0, HashSet::new()));
                *msg_count += 1;
                peers.insert(peer_ip);
            }
        }
    }
}

fn compile_as2rel_count(
    data_map: &HashMap<(u32, u32, u8), (usize, HashSet<IpAddr>)>,
) -> Vec<As2RelCount> {
    data_map
        .iter()
        .map(|((asn1, asn2, rel), (msg_count, peers))| As2RelCount {
            asn1: *asn1,
            asn2: *asn2,
            rel: *rel,
            paths_count: *msg_count,
            peers_count: peers.len(),
        })
        .collect()
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
) -> Result<(RibPeerInfo, Prefix2As, (As2Rel, As2Rel, As2Rel))> {
    // peer-stats
    let mut peer_asn_map: HashMap<IpAddr, u32> = HashMap::new();
    let mut peer_connection: HashMap<IpAddr, HashSet<u32>> = HashMap::new();
    let mut peer_v4_pfxs_map: HashMap<IpAddr, HashSet<Ipv4Net>> = HashMap::new();
    let mut peer_v6_pfxs_map: HashMap<IpAddr, HashSet<Ipv6Net>> = HashMap::new();

    // pfx2as
    let mut pfx2as_map: HashMap<(String, u32), usize> = HashMap::new();

    // as2rel
    let mut as2rel_map: HashMap<(u32, u32, u8), (usize, HashSet<IpAddr>)> = HashMap::new();
    let mut as2rel_v4_map: HashMap<(u32, u32, u8), (usize, HashSet<IpAddr>)> = HashMap::new();
    let mut as2rel_v6_map: HashMap<(u32, u32, u8), (usize, HashSet<IpAddr>)> = HashMap::new();

    for elem in BgpkitParser::new(file_url)? {
        peer_asn_map
            .entry(elem.peer_ip)
            .or_insert(elem.peer_asn.to_u32());
        if let Some(as_path) = elem.as_path.clone() {
            if let Some(u32_path) = as_path.to_u32_vec_opt(true) {
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

                // do a global and a v4/v6 specific as2rel
                for is_global in [true, false] {
                    // get tier-1 ASes list and the corresponding as2rel_map
                    let (tier1, data_map) = match is_global {
                        true => (TIER1.to_vec(), &mut as2rel_map),
                        false => match elem.prefix.prefix {
                            IpNet::V4(_) => (TIER1_V4.to_vec(), &mut as2rel_v4_map),
                            IpNet::V6(_) => (TIER1_V6.to_vec(), &mut as2rel_v6_map),
                        },
                    };
                    // update as2rel_map
                    update_as2rel_map(elem.peer_ip, &tier1, data_map, &u32_path);
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

    let as2rel_global = compile_as2rel_count(&as2rel_map);
    let as2rel_v4 = compile_as2rel_count(&as2rel_v4_map);
    let as2rel_v6 = compile_as2rel_count(&as2rel_v6_map);

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
        (
            As2Rel {
                project: project.to_string(),
                collector: collector.to_string(),
                rib_dump_url: file_url.to_string(),
                as2rel: as2rel_global,
            },
            As2Rel {
                project: project.to_string(),
                collector: collector.to_string(),
                rib_dump_url: file_url.to_string(),
                as2rel: as2rel_v4,
            },
            As2Rel {
                project: project.to_string(),
                collector: collector.to_string(),
                rib_dump_url: file_url.to_string(),
                as2rel: as2rel_v6,
            },
        ),
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
