# Peer Stats

[![Peer Stats](https://img.shields.io/badge/peer--stats-data.bgpkit.com-blue)](https://data.bgpkit.com/peer-stats)
[![AS2Rel](https://img.shields.io/badge/as2rel-data.bgpkit.com-blue)](https://data.bgpkit.com/as2rel/)
[![Pfx2As](https://img.shields.io/badge/pfx2as-data.bgpkit.com-blue)](https://data.bgpkit.com/pfx2as/)

A Rust library and CLI tool for processing BGP RIB dump files to extract peer statistics, AS relationships, and prefix-to-AS mappings.

**Hosted Results**: Processed data is available at:
- [data.bgpkit.com/peer-stats](https://data.bgpkit.com/peer-stats) - Peer statistics with daily updates dating back to 2012
- [data.bgpkit.com/as2rel](https://data.bgpkit.com/as2rel/) - AS relationship inferences
- [data.bgpkit.com/pfx2as](https://data.bgpkit.com/pfx2as/) - Prefix-to-AS mappings

## Overview

This crate provides three main processors for analyzing BGP data:

1. **Peer Stats** (`peer_stats.rs`): Collects per-peer statistics including ASN, IP, and prefix counts
2. **AS2Rel** (`as2rel.rs`): Infers AS relationships (provider-customer) using a tier-1 transit algorithm
3. **Pfx2As** (`pfx2as.rs`): Maps IP prefixes to their origin ASes

All processors use a consistent pattern: `new()` → `process_*()` → `into_*()`

## Features

- **Modular Architecture**: Each processor is self-contained in its own module
- **Tier-1 Transit Algorithm**: Distinguishes between true tier-1 ASes and candidate tier-1 ASes (Zayo 6461, Hurricane Electric 6939) to prevent over-counting downstream ASes
- **Processor Pattern**: Consistent API across all modules
- **SQLite Indexing**: Tools to index processed data into SQLite databases

## AS Relationship Inference

The AS2Rel processor uses a two-tier algorithm to infer provider-customer relationships:

- **True Tier-1 ASes** (14 networks): Always considered transit providers
  - Cogent (174), Lumen (3356), NTT (2914), Verizon (701), etc.
- **Candidate Tier-1 ASes**: Only transit providers when next hop is tier-1
  - AS 6461 (Zayo) - IPv4 and IPv6
  - AS 6939 (Hurricane Electric) - IPv6 only

This prevents over-counting downstream ASes for networks that peer extensively but don't sell transit service.

## Installation

Rust toolchain is required:

```bash
cargo install --path .
```

## Binaries

The project builds 5 binaries:

### peer-stats-single-file
Process a single RIB dump file (outputs all three data types):

```bash
peer-stats-single-file --debug http://archive.routeviews.org/route-views.sg/bgpdata/2022.02/RIBS/rib.20220205.1800.bz2
```

### peer-stats-bootstrap
Bootstrap historical data collection:

```bash
MAX_THREADS=8 peer-stats-bootstrap --output-dir ./data --ts-start 2022-01-01 --ts-end 2022-02-01
```

### peer-stats-index
Index peer statistics into SQLite:

```bash
peer-stats-index --db-path ./peer-stats.db --input-dir ./data
```

### as2rel-index
Index AS relationships into SQLite:

```bash
as2rel-index --db-path ./as2rel.db --input-dir ./data
```

### pfx2as-index
Index prefix-to-AS mappings into SQLite:

```bash
pfx2as-index --db-path ./pfx2as.db --input-dir ./data
```

## Library Usage

```rust
use peer_stats::parse_rib_file;

let (peer_stats, pfx2as, (as2rel_global, as2rel_v4, as2rel_v6)) = 
    parse_rib_file(
        "http://archive.routeviews.org/.../rib.20220205.1800.bz2",
        "route-views",
        "route-views.sg"
    ).unwrap();
```

## Data Types

### Peer Stats Output
```json
{
  "project": "route-views",
  "collector": "route-views.sg",
  "rib_dump_url": "...",
  "peers": {
    "2001:de8:4::13:6168:1": {
      "asn": 136168,
      "ip": "2001:de8:4::13:6168:1",
      "num_v4_pfxs": 0,
      "num_v6_pfxs": 40,
      "num_connected_asns": 4
    }
  }
}
```

### AS2Rel Output
```json
{
  "project": "route-views",
  "collector": "route-views.sg",
  "rib_dump_url": "...",
  "as2rel": [
    {
      "asn1": 174,
      "asn2": 13335,
      "rel": 1,
      "paths_count": 42,
      "peers_count": 3
    }
  ]
}
```

### Pfx2As Output
```json
{
  "project": "route-views",
  "collector": "route-views.sg",
  "rib_dump_url": "...",
  "pfx2as": [
    {
      "prefix": "1.1.1.0/24",
      "asn": 13335,
      "count": 5
    }
  ]
}
```

## Public Dataset

We provide a publicly available dataset at https://data.bgpkit.com/peer-stats.
We update this dataset daily and provide historical data archive back to 2012.

## License

This work is under [MIT](LICENSE) license.
