pub const TIER1: [u32; 16] = [
    6762, 12956, 2914, 3356, 6453, 701, 6461, 3257, 1299, 3491, 7018, 3320, 5511, 6830, 174, 6939,
];

pub const TIER1_V4: [u32; 15] = [
    6762, 12956, 2914, 3356, 6453, 701, 6461, 3257, 1299, 3491, 7018, 3320, 5511, 6830, 174,
];

pub const TIER1_V6: [u32; 16] = [
    6762, 12956, 2914, 3356, 6453, 701, 6461, 3257, 1299, 3491, 7018, 3320, 5511, 6830, 174, 6939,
];

use ipnet::IpNet;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct As2Rel {
    pub project: String,
    pub collector: String,
    pub rib_dump_url: String,
    /// AS relationship mapping: Vec<As2RelCount>
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

pub fn dedup_path(path: Vec<u32>) -> Vec<u32> {
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

pub fn update_as2rel_map(
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

pub fn compile_as2rel_count(
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

pub struct As2RelProcessor {
    as2rel_map: HashMap<(u32, u32, u8), (usize, HashSet<IpAddr>)>,
    as2rel_v4_map: HashMap<(u32, u32, u8), (usize, HashSet<IpAddr>)>,
    as2rel_v6_map: HashMap<(u32, u32, u8), (usize, HashSet<IpAddr>)>,
}

impl As2RelProcessor {
    pub fn new() -> Self {
        Self {
            as2rel_map: HashMap::new(),
            as2rel_v4_map: HashMap::new(),
            as2rel_v6_map: HashMap::new(),
        }
    }

    pub fn process_path(&mut self, peer_ip: IpAddr, prefix_type: IpNet, as_path: &[u32]) {
        // global as2rel (all paths)
        update_as2rel_map(peer_ip, &TIER1, &mut self.as2rel_map, as_path);

        // v4/v6 specific as2rel
        match prefix_type {
            IpNet::V4(_) => {
                update_as2rel_map(peer_ip, &TIER1_V4, &mut self.as2rel_v4_map, as_path);
            }
            IpNet::V6(_) => {
                update_as2rel_map(peer_ip, &TIER1_V6, &mut self.as2rel_v6_map, as_path);
            }
        }
    }

    pub fn into_as2rel_triple(
        self,
        project: &str,
        collector: &str,
        rib_dump_url: &str,
    ) -> (As2Rel, As2Rel, As2Rel) {
        let as2rel_global = compile_as2rel_count(&self.as2rel_map);
        let as2rel_v4 = compile_as2rel_count(&self.as2rel_v4_map);
        let as2rel_v6 = compile_as2rel_count(&self.as2rel_v6_map);

        (
            As2Rel {
                project: project.to_string(),
                collector: collector.to_string(),
                rib_dump_url: rib_dump_url.to_string(),
                as2rel: as2rel_global,
            },
            As2Rel {
                project: project.to_string(),
                collector: collector.to_string(),
                rib_dump_url: rib_dump_url.to_string(),
                as2rel: as2rel_v4,
            },
            As2Rel {
                project: project.to_string(),
                collector: collector.to_string(),
                rib_dump_url: rib_dump_url.to_string(),
                as2rel: as2rel_v6,
            },
        )
    }
}

impl Default for As2RelProcessor {
    fn default() -> Self {
        Self::new()
    }
}
