use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

pub struct Pfx2AsProcessor {
    pfx2as_map: HashMap<(String, u32), usize>,
}

impl Pfx2AsProcessor {
    pub fn new() -> Self {
        Self {
            pfx2as_map: HashMap::new(),
        }
    }

    pub fn record(&mut self, prefix: String, asn: u32) {
        let count = self.pfx2as_map.entry((prefix, asn)).or_insert(0);
        *count += 1;
    }

    pub fn into_prefix2as(self, project: &str, collector: &str, rib_dump_url: &str) -> Prefix2As {
        let pfx2as = self
            .pfx2as_map
            .into_iter()
            .map(|((prefix, asn), count)| Prefix2AsCount { prefix, asn, count })
            .collect();

        Prefix2As {
            project: project.to_string(),
            collector: collector.to_string(),
            rib_dump_url: rib_dump_url.to_string(),
            pfx2as,
        }
    }
}

impl Default for Pfx2AsProcessor {
    fn default() -> Self {
        Self::new()
    }
}
