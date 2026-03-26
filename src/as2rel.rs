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
/// These are only considered transit providers if their next hop is a TRUE_TIER1 AS.
/// This prevents over-counting downstream ASes for networks that don't actually sell transit.
pub const CANDIDATE_TIER1_V4: [u32; 1] = [
    6461, // Zayo - only provides transit if connecting to a true tier-1
];

/// Candidate Tier-1 ASes for IPv6 that may provide transit.
/// Same logic as CANDIDATE_TIER1_V4 but for IPv6 paths.
pub const CANDIDATE_TIER1_V6: [u32; 2] = [
    6461, // Zayo - IPv6
    6939, // Hurricane Electric (IPv6 only) - only provides transit if connecting to a true tier-1
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
/// 1. Look from origin towards collector for the first tier-1 AS (true or candidate)
/// 2. If it's a TRUE_TIER1 AS: return it as the transit point
/// 3. If it's a CANDIDATE_TIER1 AS: check if its next hop is a TRUE_TIER1 AS
///    - If yes: return it as the transit point
///    - If no: return None (stop here, don't look further)
///
/// This prevents over-counting downstream ASes. For Hurricane Electric and Zayo,
/// we only consider them valid transit providers if they're directly adjacent to
/// another tier-1 AS. Otherwise, we don't mark any p2c relationships for the path.
pub fn find_transit_point(
    as_path: &[u32],
    true_tier1: &[u32],
    candidate_tier1: &[u32],
) -> Option<usize> {
    let true_tier1_set: HashSet<u32> = true_tier1.iter().copied().collect();
    let candidate_set: HashSet<u32> = candidate_tier1.iter().copied().collect();

    for (i, asn) in as_path.iter().enumerate() {
        // True tier-1: always valid transit
        if true_tier1_set.contains(asn) {
            return Some(i);
        }

        // Candidate tier-1: only valid if next hop is a TRUE_TIER1 AS
        if candidate_set.contains(asn) {
            if i + 1 < as_path.len() && true_tier1_set.contains(&as_path[i + 1]) {
                // Candidate with tier-1 next hop - valid transit point
                return Some(i);
            } else {
                // Candidate without tier-1 next hop - STOP, don't look further
                return None;
            }
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
    candidate_tier1: &[u32],
    data_map: &mut HashMap<(u32, u32, u8), (usize, HashSet<IpAddr>)>,
    // input AS path must be from collector ([0]) to origin ([last])
    original_as_path: &[u32],
) {
    let mut as_path = original_as_path.to_vec();

    // Count peer relationships first
    count_peer_relationships(peer_ip, &as_path, data_map);

    // Reverse to process from origin towards collector
    as_path.reverse();

    // Find the transit point using the tier-1 algorithm
    if let Some(transit_idx) = find_transit_point(&as_path, true_tier1, candidate_tier1) {
        // Mark all ASes from origin up to (but not including) the transit point
        // as customer -> provider relationships.
        // We know all these providers are valid because find_transit_point only
        // returns a transit point if it's a true tier-1 or candidate with tier-1 next hop.
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

/// Combine two relationship maps (v4 and v6) into a single global map.
fn combine_as2rel_maps(
    v4_map: &HashMap<(u32, u32, u8), (usize, HashSet<IpAddr>)>,
    v6_map: &HashMap<(u32, u32, u8), (usize, HashSet<IpAddr>)>,
) -> Vec<As2RelCount> {
    let mut combined: HashMap<(u32, u32, u8), (usize, HashSet<IpAddr>)> = HashMap::new();

    // Add all v4 entries
    for ((asn1, asn2, rel), (count, peers)) in v4_map.iter() {
        let entry = combined
            .entry((*asn1, *asn2, *rel))
            .or_insert((0, HashSet::new()));
        entry.0 += *count;
        entry.1.extend(peers.iter().copied());
    }

    // Add all v6 entries
    for ((asn1, asn2, rel), (count, peers)) in v6_map.iter() {
        let entry = combined
            .entry((*asn1, *asn2, *rel))
            .or_insert((0, HashSet::new()));
        entry.0 += *count;
        entry.1.extend(peers.iter().copied());
    }

    compile_as2rel_count(&combined)
}

/// Processor for collecting AS relationship data from BGP RIB dumps.
///
/// Uses the tier-1 transit algorithm to distinguish between:
/// - True tier-1 ASes that always provide transit
/// - Candidate tier-1 ASes (Zayo, Hurricane Electric) that only provide transit
///   when connecting to a true tier-1
///
/// This prevents over-counting downstream ASes for networks that don't actually
/// sell transit service in the traditional sense.
pub struct As2RelProcessor {
    as2rel_v4_map: HashMap<(u32, u32, u8), (usize, HashSet<IpAddr>)>,
    as2rel_v6_map: HashMap<(u32, u32, u8), (usize, HashSet<IpAddr>)>,
}

impl As2RelProcessor {
    pub fn new() -> Self {
        Self {
            as2rel_v4_map: HashMap::new(),
            as2rel_v6_map: HashMap::new(),
        }
    }

    /// Process a single AS path and update relationship statistics.
    ///
    /// For IPv4: Uses TRUE_TIER1 and CANDIDATE_TIER1_V4
    /// For IPv6: Uses TRUE_TIER1 and CANDIDATE_TIER1_V6
    pub fn process_path(&mut self, peer_ip: IpAddr, prefix_type: IpNet, as_path: &[u32]) {
        match prefix_type {
            IpNet::V4(_) => {
                update_as2rel_map(
                    peer_ip,
                    &TRUE_TIER1,
                    &CANDIDATE_TIER1_V4,
                    &mut self.as2rel_v4_map,
                    as_path,
                );
            }
            IpNet::V6(_) => {
                update_as2rel_map(
                    peer_ip,
                    &TRUE_TIER1,
                    &CANDIDATE_TIER1_V6,
                    &mut self.as2rel_v6_map,
                    as_path,
                );
            }
        }
    }

    /// Convert collected data into As2Rel structs.
    /// Global as2rel is derived from combining v4 and v6 results.
    pub fn into_as2rel_triple(
        self,
        project: &str,
        collector: &str,
        rib_dump_url: &str,
    ) -> (As2Rel, As2Rel, As2Rel) {
        let as2rel_v4 = compile_as2rel_count(&self.as2rel_v4_map);
        let as2rel_v6 = compile_as2rel_count(&self.as2rel_v6_map);

        // Combine v4 and v6 for global results
        let as2rel_global = combine_as2rel_maps(&self.as2rel_v4_map, &self.as2rel_v6_map);

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

    #[test]
    fn test_find_transit_point_true_tier1() {
        // True tier-1 should always be transit point
        let path = vec![100, 200, 174, 300]; // 174 (Cogent) is true tier-1
        let result = find_transit_point(&path, &TRUE_TIER1, &CANDIDATE_TIER1_V4);
        assert_eq!(result, Some(2)); // Index of 174
    }

    #[test]
    fn test_find_transit_point_candidate_with_tier1_next() {
        // 6461 (Zayo) with tier-1 next hop should be transit
        let path = vec![100, 200, 6461, 174, 300]; // 6461 -> 174 (tier-1)
        let result = find_transit_point(&path, &TRUE_TIER1, &CANDIDATE_TIER1_V4);
        assert_eq!(result, Some(2)); // Index of 6461
    }

    #[test]
    fn test_find_transit_point_candidate_without_tier1_next() {
        // 6461 without tier-1 next hop should NOT be transit
        let path = vec![100, 200, 6461, 300, 400]; // 6461 -> 300 (not tier-1)
        let result = find_transit_point(&path, &TRUE_TIER1, &CANDIDATE_TIER1_V4);
        assert_eq!(result, None); // No valid transit point
    }

    #[test]
    fn test_find_transit_point_candidate_blocks_further_search() {
        // When we encounter a candidate without tier-1 next hop, we STOP
        // We do NOT continue looking for true tier-1s further down the path
        let path = vec![100, 200, 6461, 300, 174, 400];
        // 6461 at index 2 doesn't transit (300 not tier-1)
        // We STOP here and return None, even though 174 is a true tier-1 later
        let result = find_transit_point(&path, &TRUE_TIER1, &CANDIDATE_TIER1_V4);
        assert_eq!(result, None); // No valid transit point - candidate blocks search
    }

    #[test]
    fn test_find_transit_point_he_with_tier1_next_v6() {
        // HE (6939) with tier-1 next hop should be valid transit (IPv6)
        let path = vec![100, 200, 6939, 174, 300]; // 6939 -> 174 (tier-1)
        let result = find_transit_point(&path, &TRUE_TIER1, &CANDIDATE_TIER1_V6);
        assert_eq!(result, Some(2)); // Index of 6939
    }

    #[test]
    fn test_find_transit_point_he_without_tier1_next_v6() {
        // HE (6939) without tier-1 next hop should NOT be transit (IPv6)
        // We STOP at 6939 and don't look further
        let path = vec![100, 200, 6939, 300, 400]; // 6939 -> 300 (not tier-1)
        let result = find_transit_point(&path, &TRUE_TIER1, &CANDIDATE_TIER1_V6);
        assert_eq!(result, None); // No valid transit point
    }

    #[test]
    fn test_he_not_candidate_in_v4() {
        // HE (6939) should NOT be a valid candidate in IPv4
        // Since 6939 is not in CANDIDATE_TIER1_V4, we skip it and continue
        let path = vec![100, 200, 6939, 174, 300]; // 6939 -> 174 (tier-1)
        let result = find_transit_point(&path, &TRUE_TIER1, &CANDIDATE_TIER1_V4);
        // In v4, 6939 is not a candidate, so we skip it and find 174
        assert_eq!(result, Some(3)); // Index of 174
    }

    #[test]
    fn test_candidate_blocks_when_next_is_candidate_not_true() {
        // When candidate's next hop is another candidate (not true tier-1), we STOP
        // We do NOT continue to find if that next candidate is valid
        let path = vec![100, 6461, 6939, 174, 300]; // 6461 -> 6939 -> 174
                                                    // 6461 is candidate, next hop 6939 is also candidate (not true tier-1)
                                                    // We STOP at 6461 and return None
        let result = find_transit_point(&path, &TRUE_TIER1, &CANDIDATE_TIER1_V6);
        assert_eq!(result, None); // 6461 blocks, we don't check 6939
    }

    #[test]
    fn test_he_as_first_candidate_with_tier1_next() {
        // HE as the first tier-1 encountered with true tier-1 next hop
        let path = vec![100, 6939, 174, 300]; // 6939 -> 174 (tier-1)
        let result = find_transit_point(&path, &TRUE_TIER1, &CANDIDATE_TIER1_V6);
        assert_eq!(result, Some(1)); // Index of 6939
    }
}
