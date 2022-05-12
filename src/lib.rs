#![allow(dead_code)]
use bgpkit_parser::{AsPathSegment, BgpkitParser};
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use ipnetwork::IpNetwork;
use serde::Serialize;
use anyhow::Result;

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
pub fn parse_rib_file(file_url: &str, project: &str, collector: &str) -> Result<RibPeerInfo> {

    let mut peer_asn_map: HashMap<IpAddr, u32> = HashMap::new();
    let mut peer_connection: HashMap<IpAddr, HashSet<u32>> = HashMap::new();
    let mut peer_v4_pfxs_map: HashMap<IpAddr, HashSet<IpNetwork>> = HashMap::new();
    let mut peer_v6_pfxs_map: HashMap<IpAddr, HashSet<IpNetwork>> = HashMap::new();

    for elem in BgpkitParser::new(file_url)? {
        peer_asn_map.entry(elem.peer_ip).or_insert(elem.peer_asn.asn);
        if let Some(as_path) = elem.as_path.clone() {
            match as_path.clone().segments.get(0) {
                Some(path) => {
                    match path {
                        AsPathSegment::AsSequence(a) => {
                            match a.get(1){
                                None => {}
                                Some(asn) => {
                                    peer_connection.entry(elem.peer_ip).or_insert(HashSet::<u32>::new()).insert(asn.asn);
                                }
                            };
                        }
                        _ => {}
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
                    .or_insert(HashSet::<IpNetwork>::new())
                    .insert(elem.prefix.prefix);
            }
            false => {
                peer_v6_pfxs_map.entry(elem.peer_ip)
                    .or_insert(HashSet::<IpNetwork>::new())
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
        let ip_clone = ip.clone();
        let asn_clone = asn.clone();
        peer_info_map.insert(
            ip_clone.clone(), PeerInfo{
                ip: ip_clone,
                asn: asn_clone,
                num_v4_pfxs,
                num_v6_pfxs,
                num_connected_asns: num_connected_asn,
            }
        );
    }


    Ok(
        RibPeerInfo {
            project: project.to_string(),
            collector: collector.to_string(),
            rib_dump_url: file_url.to_string(),
            peers: peer_info_map
        }
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
        let info = parse_rib_file("http://archive.routeviews.org/route-views.sg/bgpdata/2022.02/RIBS/rib.20220205.1800.bz2",
        "route-views", "route-views.sg");
        serde_json::to_writer_pretty(&File::create("peer_info_example.json").unwrap(), &json!(info)).unwrap();
        // dbg!(info);
        info!("finished");
    }
}