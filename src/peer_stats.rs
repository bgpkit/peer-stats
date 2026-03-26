use ipnet::{Ipv4Net, Ipv6Net};
use serde::Serialize;
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

pub struct PeerStatsProcessor {
    peer_asn_map: HashMap<IpAddr, u32>,
    peer_connection: HashMap<IpAddr, HashSet<u32>>,
    peer_v4_pfxs_map: HashMap<IpAddr, HashSet<Ipv4Net>>,
    peer_v6_pfxs_map: HashMap<IpAddr, HashSet<Ipv6Net>>,
}

impl PeerStatsProcessor {
    pub fn new() -> Self {
        Self {
            peer_asn_map: HashMap::new(),
            peer_connection: HashMap::new(),
            peer_v4_pfxs_map: HashMap::new(),
            peer_v6_pfxs_map: HashMap::new(),
        }
    }

    pub fn process_element(
        &mut self,
        peer_ip: IpAddr,
        peer_asn: u32,
        prefix_v4: Option<Ipv4Net>,
        prefix_v6: Option<Ipv6Net>,
        connected_asn: Option<u32>,
    ) {
        self.peer_asn_map.entry(peer_ip).or_insert(peer_asn);

        if let Some(asn) = connected_asn {
            self.peer_connection.entry(peer_ip).or_default().insert(asn);
        }

        if let Some(net) = prefix_v4 {
            self.peer_v4_pfxs_map
                .entry(peer_ip)
                .or_default()
                .insert(net);
        }

        if let Some(net) = prefix_v6 {
            self.peer_v6_pfxs_map
                .entry(peer_ip)
                .or_default()
                .insert(net);
        }
    }

    pub fn into_peer_info(self, project: &str, collector: &str, rib_dump_url: &str) -> RibPeerInfo {
        let mut peer_info_map: HashMap<IpAddr, PeerInfo> = HashMap::new();

        for (ip, asn) in self.peer_asn_map {
            let num_v4_pfxs = self.peer_v4_pfxs_map.get(&ip).map_or(0, |s| s.len());
            let num_v6_pfxs = self.peer_v6_pfxs_map.get(&ip).map_or(0, |s| s.len());
            let num_connected_asns = self.peer_connection.get(&ip).map_or(0, |s| s.len());

            peer_info_map.insert(
                ip,
                PeerInfo {
                    ip,
                    asn,
                    num_v4_pfxs,
                    num_v6_pfxs,
                    num_connected_asns,
                },
            );
        }

        RibPeerInfo {
            project: project.to_string(),
            collector: collector.to_string(),
            rib_dump_url: rib_dump_url.to_string(),
            peers: peer_info_map,
        }
    }
}

impl Default for PeerStatsProcessor {
    fn default() -> Self {
        Self::new()
    }
}
