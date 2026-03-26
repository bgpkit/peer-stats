#![allow(dead_code)]

pub mod as2rel;
pub mod peer_stats;
pub mod pfx2as;

// Re-export tier-1 constants from as2rel
pub use as2rel::{TIER1, TIER1_V4, TIER1_V6};

// Re-export types from their respective modules
pub use as2rel::{As2Rel, As2RelCount};
pub use peer_stats::{PeerInfo, RibPeerInfo};
pub use pfx2as::{Prefix2As, Prefix2AsCount};

// Re-export processors
pub use as2rel::{dedup_path, As2RelProcessor};
pub use peer_stats::PeerStatsProcessor;
pub use pfx2as::Pfx2AsProcessor;

use anyhow::Result;
use bgpkit_parser::BgpkitParser;
use ipnet::IpNet;

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
    let mut peer_stats_collector = PeerStatsProcessor::new();
    let mut pfx2as_collector = Pfx2AsProcessor::new();
    let mut as2rel_collector = As2RelProcessor::new();

    for elem in BgpkitParser::new(file_url)? {
        let peer_ip = elem.peer_ip;
        let peer_asn = elem.peer_asn.to_u32();
        let prefix = elem.prefix.prefix;

        // Extract prefix info
        let (prefix_v4, prefix_v6) = match prefix {
            IpNet::V4(net) => (Some(net), None),
            IpNet::V6(net) => (None, Some(net)),
        };

        // Process AS path data
        let mut connected_asn = None;
        if let Some(as_path) = elem.as_path.clone() {
            if let Some(u32_path) = as_path.to_u32_vec_opt(true) {
                // Get connected ASN (second hop in path)
                connected_asn = u32_path.get(1).copied();

                // Get origin ASN for pfx2as
                if let Some(origin_asn) = u32_path.last().copied() {
                    pfx2as_collector.record(prefix.to_string(), origin_asn);
                }

                // Process AS relationships
                as2rel_collector.process_path(peer_ip, prefix, &u32_path);
            }
        }

        // Update peer stats
        peer_stats_collector.process_element(
            peer_ip,
            peer_asn,
            prefix_v4,
            prefix_v6,
            connected_asn,
        );

        drop(elem);
    }

    let peer_info = peer_stats_collector.into_peer_info(project, collector, file_url);
    let pfx2as = pfx2as_collector.into_prefix2as(project, collector, file_url);
    let as2rel_triple = as2rel_collector.into_as2rel_triple(project, collector, file_url);

    Ok((peer_info, pfx2as, as2rel_triple))
}

#[cfg(test)]
mod tests {
    use crate::as2rel::dedup_path;
    use crate::parse_rib_file;
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
