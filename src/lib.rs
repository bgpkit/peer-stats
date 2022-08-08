#![allow(dead_code)]
use bgpkit_parser::{AsPathSegment, BgpkitParser};
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use ipnetwork::IpNetwork;
use serde::Serialize;
use anyhow::Result;
use itertools::Itertools;
use tracing::info;

#[derive(Debug, Clone, Serialize)]
pub struct RibPeerInfo {
    project: String,
    collector: String,
    rib_dump_url: String,
    peers: HashMap<IpAddr, PeerInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PeerInfo {
    ip: IpAddr,
    asn: u32,
    num_v4_pfxs: usize,
    num_v6_pfxs: usize,
    num_connected_asns: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Prefix2As {
    project: String,
    collector: String,
    rib_dump_url: String,
    /// prefix to as mapping: <prefix, <asn, count>>
    pfx2as: Vec<Prefix2AsCount>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Prefix2AsCount {
    prefix: String,
    asn: u32,
    count: usize,
}


#[derive(Debug, Clone, Serialize)]
pub struct As2Rel {
    project: String,
    collector: String,
    rib_dump_url: String,
    /// prefix to as mapping: <prefix, <asn, count>>
    as2rel: Vec<As2RelCount>,
}

#[derive(Debug, Clone, Serialize)]
pub struct As2RelCount {
    asn1: u32,
    asn2: u32,
    /// 1 - asn1 is upstream of asn2, 2 - peer, 0 - unknown
    rel: u8,
    count: usize,
}

const TIER1: [u32; 17] = [ 6762, 12956, 2914, 3356, 6453, 1239, 701 , 6461, 3257, 1299, 3491, 7018, 3320, 5511, 6830, 174 , 6939, ];

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
pub fn parse_rib_file(file_url: &str, project: &str, collector: &str) -> Result<(RibPeerInfo, Prefix2As, As2Rel) > {

    // peer-stats
    let mut peer_asn_map: HashMap<IpAddr, u32> = HashMap::new();
    let mut peer_connection: HashMap<IpAddr, HashSet<u32>> = HashMap::new();
    let mut peer_v4_pfxs_map: HashMap<IpAddr, HashSet<IpNetwork>> = HashMap::new();
    let mut peer_v6_pfxs_map: HashMap<IpAddr, HashSet<IpNetwork>> = HashMap::new();

    // pfx2as
    let mut pfx2as_map: HashMap<(String, u32), usize> = HashMap::new();

    // as2rel
    let mut as2rel_map: HashMap<(u32, u32, u8), usize> = HashMap::new();

    for (elem_count, elem) in (BgpkitParser::new(file_url)?).into_iter().enumerate() {

        if elem_count % 1000 == 0 {
            info!("{}", elem);
        }

        peer_asn_map.entry(elem.peer_ip).or_insert(elem.peer_asn.asn);
        if let Some(as_path) = elem.as_path.clone() {
            match as_path.clone().segments.get(0) {
                Some(path) => {
                    if let AsPathSegment::AsSequence(a) = path {
                        let mut u32_path = a.iter().map(|x| x.asn.clone()).collect::<Vec<u32>>();
                        // peer-stats
                        match u32_path.get(1){
                            None => {}
                            Some(asn) => {
                                peer_connection.entry(elem.peer_ip).or_insert(HashSet::<u32>::new()).insert(asn.clone());
                            }
                        };

                        // pfx2as
                        match u32_path.last() {
                            None => {}
                            Some(asn) => {
                                let prefix = elem.prefix.to_string();
                                let count = pfx2as_map.entry((prefix, asn.clone())).or_insert(0);
                                *count += 1;
                            }
                        }

                        // as2rel

                        // counting peer relationships
                        for (asn1, asn2) in u32_path.iter().tuple_windows::<(&u32, &u32)>(){
                            let count = as2rel_map.entry((*asn1, *asn2, 0)).or_insert(0);
                            *count += 1;
                        }

                        let contains_tier1 = u32_path.iter().any(|x| TIER1.contains(x));

                        u32_path.reverse();
                        if contains_tier1 {
                            let mut first_tier1: usize = usize::MAX;
                            for (i, asn) in u32_path.iter().enumerate() {
                                if TIER1.contains(asn) && first_tier1 == usize::MAX {
                                    first_tier1 = i;
                                    break
                                }
                            }

                            // origin to first tier 1
                            if first_tier1<u32_path.len()-1{
                                for i in 0..first_tier1 {
                                    let (asn1, asn2) = (u32_path.get(i).unwrap(), u32_path.get(i+1).unwrap());
                                    let count = as2rel_map.entry((*asn2, *asn1, 1)).or_insert(0);
                                    *count+=1;
                                }
                            }
                        }
                    }
                }
                _ => {
                    // panic!("{}", as_path);
                    continue;
                }
            };
        }

        match elem.prefix.prefix.is_ipv4() {
            true => {
                peer_v4_pfxs_map.entry(elem.peer_ip)
                    .or_insert_with(HashSet::<IpNetwork>::new)
                    .insert(elem.prefix.prefix);
            }
            false => {
                peer_v6_pfxs_map.entry(elem.peer_ip)
                    .or_insert_with(HashSet::<IpNetwork>::new)
                    .insert(elem.prefix.prefix);
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
            PeerInfo{
                ip: ip_clone,
                asn: asn_clone,
                num_v4_pfxs,
                num_v6_pfxs,
                num_connected_asns: num_connected_asn,
            }
        );
    }

    let pfx2as = pfx2as_map.into_iter().map(|((prefix, asn), count)|{
        Prefix2AsCount {
            prefix,
            asn,
            count
        }
    }).collect();

    let as2rel = as2rel_map.into_iter().map(|((asn1, asn2, rel), count)| {
        As2RelCount{
            asn1,
            asn2,
            rel,
            count
        }
    }).collect();

    Ok(
        (
            RibPeerInfo {
            project: project.to_string(),
            collector: collector.to_string(),
            rib_dump_url: file_url.to_string(),
            peers: peer_info_map
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
                as2rel
            }
        )
    )
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use serde_json::json;
    use tracing::{info, Level};
    use super::*;

    #[test]
    fn test_read_rib() {
        tracing_subscriber::fmt()
            // filter spans/events with level TRACE or higher.
            .with_max_level(Level::INFO)
            .init();
        info!("start");
        let (peer_stats, pfx2as, as2rel) = parse_rib_file("http://archive.routeviews.org/route-views.soxrs/bgpdata/2022.08/RIBS/rib.20220808.1400.bz2",
        "route-views", "route-views.sg").unwrap();
        serde_json::to_writer_pretty(&File::create("peer_info_example.json").unwrap(), &json!(peer_stats)).unwrap();
        serde_json::to_writer_pretty(&File::create("pfx2as_example.json").unwrap(), &json!(pfx2as)).unwrap();
        serde_json::to_writer_pretty(&File::create("as2rel_example.json").unwrap(), &json!(as2rel)).unwrap();
        info!("finished");
    }
}