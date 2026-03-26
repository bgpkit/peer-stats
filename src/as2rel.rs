use ipnet::IpNet;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;

/// True Tier-1 ASes that always provide transit service.
/// These are the major transit providers that definitively sell upstream connectivity.
pub const TRUE_TIER1: [u32; 14] = [
    6762,  // Sparkle
    12956, // Telefonica
    2914,  // NTT
    3356,  // Lumen
    6453,  // TATA
    701,   // Verizon
    3257,  // GTT
    1299,  // Telia
    3491,  // PCCW
    7018,  // AT&T
    3320,  // DTAG
    5511,  // Orange
    6830,  // Liberty Global
    174,   // Cogent
];

/// Candidate Tier-1 ASes for IPv4 that may provide transit.
/// These are only considered transit providers if their next hop is another tier-1 AS.
/// This prevents over-counting downstream ASes for networks that don't actually sell transit.
pub const CANDIDATE_TIER1_V4: [u32; 1] = [
    6461, // Zayo - only provides transit if connecting to another tier-1
];

/// Candidate Tier-1 ASes for IPv6 that may provide transit.
/// Same logic as CANDIDATE_TIER1_V4 but for IPv6 paths.
pub const CANDIDATE_TIER1_V6: [u32; 2] = [
    6461, // Zayo - IPv6
    6939, // Hurricane Electric (IPv6 only) - only provides transit if connecting to another tier-1
];

/// Combined Tier-1 lists for backward compatibility and global processing.
/// These include both true tier-1s and candidates.
pub const TIER1: [u32; 16] = [
    6762, 12956, 2914, 3356, 6453, 701, 6461, 3257, 1299, 3491, 7018, 3320, 5511, 6830, 174, 6939,
];

pub const TIER1_V4: [u32; 15] = [
    6762, 12956, 2914, 3356, 6453, 701, 6461, 3257, 1299, 3491, 7018, 3320, 5511, 6830, 174,
];

pub const TIER1_V6: [u32; 16] = [
    6762, 12956, 2914, 3356, 6453, 701, 6461, 3257, 1299, 3491, 7018, 3320, 5511, 6830, 174, 6939,
];

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
    /// 0 - adjacency (undirected), 1 - asn1 is upstream of asn2
    pub rel: u8,
    /// number of paths having this relationship
    pub paths_count: usize,
    /// number of peers seeing this relationship
    pub peers_count: usize,
}

/// Remove consecutive duplicate ASNs from an AS path.
/// This handles path prepending where the same AS appears multiple times in a row.
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

/// Count peer-to-peer relationships (adjacent ASes in path).
/// This records all direct connections between ASes with rel=0 (unknown).
fn count_peer_relationships(
    peer_ip: IpAddr,
    as_path: &[u32],
    data_map: &mut HashMap<(u32, u32, u8), (usize, HashSet<IpAddr>)>,
) {
    for (asn1, asn2) in as_path.iter().tuple_windows::<(&u32, &u32)>() {
        let (msg_count, peers) = data_map
            .entry((*asn1, *asn2, 0))
            .or_insert((0, HashSet::new()));
        *msg_count += 1;
        peers.insert(peer_ip);
    }
}

/// Determine the transit point in an AS path using the tier-1 algorithm.
///
/// Algorithm:
/// 1. Look for the first TRUE_TIER1 AS - this is always a valid transit provider
/// 2. If we encounter a CANDIDATE_TIER1 AS first, check if its next hop is another tier-1 AS
///    - If yes: this candidate is a valid transit provider
///    - If no: continue looking for the next tier-1
/// 3. Return the index of the first valid transit point, or None if none found
///
/// This prevents over-counting downstream ASes for networks like Zayo (6461) and
/// Hurricane Electric (6939) that don't actually sell transit service unless they're
/// connecting to another tier-1.
pub fn find_transit_point(
    as_path: &[u32],
    true_tier1: &[u32],
    all_tier1_set: &HashSet<u32>,
) -> Option<usize> {
    for (i, asn) in as_path.iter().enumerate() {
        // True tier-1: always valid transit
        if true_tier1.contains(asn) {
            return Some(i);
        }

        // Candidate tier-1: only valid if next hop is also tier-1
        if all_tier1_set.contains(asn)
            && !true_tier1.contains(asn)
            && i + 1 < as_path.len()
            && all_tier1_set.contains(&as_path[i + 1])
        {
            return Some(i);
        }
    }

    None
}

/// Update AS relationship map with provider-customer relationships.
///
/// Uses the tier-1 transit algorithm to determine which ASes are upstream providers.
/// Only ASes between the origin and the first valid transit point are marked as
/// customer->provider relationships (rel=1).
pub fn update_as2rel_map(
    peer_ip: IpAddr,
    true_tier1: &[u32],
    all_tier1: &[u32],
    data_map: &mut HashMap<(u32, u32, u8), (usize, HashSet<IpAddr>)>,
    // input AS path must be from collector ([0]) to origin ([last])
    original_as_path: &[u32],
) {
    let mut as_path = original_as_path.to_vec();

    // Count peer relationships first
    count_peer_relationships(peer_ip, &as_path, data_map);

    // Reverse to process from origin towards collector
    as_path.reverse();

    // Build tier-1 lookup set once
    let all_tier1_set: HashSet<u32> = all_tier1.iter().copied().collect();

    // Find the transit point using the tier-1 algorithm
    if let Some(transit_idx) = find_transit_point(&as_path, true_tier1, &all_tier1_set) {
        // Mark all ASes from origin up to (but not including) the transit point
        // as customer -> provider relationships
        if transit_idx < as_path.len() - 1 {
            for i in 0..transit_idx {
                let customer = as_path[i];
                let provider = as_path[i + 1];
                let (msg_count, peers) = data_map
                    .entry((provider, customer, 1))
                    .or_insert((0, HashSet::new()));
                *msg_count += 1;
                peers.insert(peer_ip);
            }
        }
    }
}

/// Compile raw relationship data into As2RelCount structs.
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

/// Processor for collecting AS relationship data from BGP RIB dumps.
///
/// Uses the tier-1 transit algorithm to distinguish between:
/// - True tier-1 ASes that always provide transit
/// - Candidate tier-1 ASes (Zayo, Hurricane Electric) that only provide transit
///   when connecting to another tier-1
///
/// This prevents over-counting downstream ASes for networks that don't actually
/// sell transit service in the traditional sense.
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

    /// Process a single AS path and update relationship statistics.
    ///
    /// For IPv4: Uses TRUE_TIER1 and CANDIDATE_TIER1_V4
    /// For IPv6: Uses TRUE_TIER1 and CANDIDATE_TIER1_V6
    /// Global: Uses combined TIER1 lists
    pub fn process_path(&mut self, peer_ip: IpAddr, prefix_type: IpNet, as_path: &[u32]) {
        // Global as2rel (all paths) - uses old algorithm for compatibility
        // Uses combined TIER1 list without candidate validation
        update_as2rel_map(peer_ip, &TIER1, &TIER1, &mut self.as2rel_map, as_path);

        // v4/v6 specific as2rel - uses new tier-1 transit algorithm
        match prefix_type {
            IpNet::V4(_) => {
                update_as2rel_map(
                    peer_ip,
                    &TRUE_TIER1,
                    &TIER1_V4,
                    &mut self.as2rel_v4_map,
                    as_path,
                );
            }
            IpNet::V6(_) => {
                update_as2rel_map(
                    peer_ip,
                    &TRUE_TIER1,
                    &TIER1_V6,
                    &mut self.as2rel_v6_map,
                    as_path,
                );
            }
        }
    }

    /// Convert collected data into As2Rel structs.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_find_transit_point_true_tier1() {
        // True tier-1 should always be transit point
        let path = vec![100, 200, 174, 300]; // 174 (Cogent) is true tier-1
        let all_tier1_set: HashSet<u32> = TIER1.iter().copied().collect();
        let result = find_transit_point(&path, &TRUE_TIER1, &all_tier1_set);
        assert_eq!(result, Some(2)); // Index of 174
    }

    #[test]
    fn test_find_transit_point_candidate_with_tier1_next() {
        // 6461 (Zayo) with tier-1 next hop should be transit
        let path = vec![100, 200, 6461, 174, 300]; // 6461 -> 174 (tier-1)
        let all_tier1_set: HashSet<u32> = TIER1_V4.iter().copied().collect();
        let result = find_transit_point(&path, &TRUE_TIER1, &all_tier1_set);
        assert_eq!(result, Some(2)); // Index of 6461
    }

    #[test]
    fn test_find_transit_point_candidate_without_tier1_next() {
        // 6461 without tier-1 next hop should NOT be transit
        let path = vec![100, 200, 6461, 300, 400]; // 6461 -> 300 (not tier-1)
        let all_tier1_set: HashSet<u32> = TIER1_V4.iter().copied().collect();
        let result = find_transit_point(&path, &TRUE_TIER1, &all_tier1_set);
        assert_eq!(result, None); // No valid transit point
    }

    #[test]
    fn test_find_transit_point_true_tier1_after_candidate() {
        // True tier-1 after candidate that doesn't transit
        let path = vec![100, 200, 6461, 300, 174, 400];
        // 6461 at index 2 doesn't transit (300 not tier-1)
        // 174 at index 4 is true tier-1
        let all_tier1_set: HashSet<u32> = TIER1_V4.iter().copied().collect();
        let result = find_transit_point(&path, &TRUE_TIER1, &all_tier1_set);
        assert_eq!(result, Some(4)); // Index of 174
    }

    #[test]
    fn test_find_transit_point_he_with_tier1_next_v6() {
        // HE (6939) with tier-1 next hop should be valid transit (IPv6)
        let path = vec![100, 200, 6939, 174, 300]; // 6939 -> 174 (tier-1)
        let all_tier1_set: HashSet<u32> = TIER1_V6.iter().copied().collect();
        let result = find_transit_point(&path, &TRUE_TIER1, &all_tier1_set);
        assert_eq!(result, Some(2)); // Index of 6939
    }

    #[test]
    fn test_find_transit_point_he_without_tier1_next_v6() {
        // HE (6939) without tier-1 next hop should NOT be transit (IPv6)
        let path = vec![100, 200, 6939, 300, 400]; // 6939 -> 300 (not tier-1)
        let all_tier1_set: HashSet<u32> = TIER1_V6.iter().copied().collect();
        let result = find_transit_point(&path, &TRUE_TIER1, &all_tier1_set);
        assert_eq!(result, None); // No valid transit point
    }
}
