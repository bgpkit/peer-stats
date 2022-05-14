# Peer Information Collector

`peer-stats` is a tool that collects BGP route collector peer information from the
archived MRT RIB dump files. The basic idea is to scan through all entries in a given
RIB dump file and aggregate information on per-peer basis.

## Metrics

We collect the following information and metrics for each peer in a table dump file:

- `asn`: peer ASN
- `ip`: peer IP
- `num_v4_pfxs`: the number of IPv4 IP prefixes from this peer
- `num_v6_pfxs`: the number of IPv6 IP prefixes from this peer
- `num_connected_asns`: the number of unique next-hop ASNs connected to this peer

## Public Dataset

We provide a publicly available dataset at https://data.bgpkit.com/peer-stats. 
We update this dataset daily and also provide historical data archive back to 2012.

## Usage

###  Installation

Rust toolchain is required for installing this tool.

Run the following command to install the compiled `peer-stats` tool to `~.cargo/bin/peer-stats`.
```bash
cargo install --path .
```


### Single-file Processing
```text
(venv) ➜  peer-stats git:(main) ✗ peer-stats --help
peer-stats 0.1.0
peer-stats is a CLI tool that collects peer information from a given RIB dump file

USAGE:
    peer-stats [FLAGS] <rib-file>

FLAGS:
        --debug      whether to print debug
    -h, --help       Prints help information
    -V, --version    Prints version information

ARGS:
    <rib-file>    File path to a MRT file, local or remote
```

Example:
```bash
peer-stats --debug http://archive.routeviews.org/route-views.sg/bgpdata/2022.02/RIBS/rib.20220205.1800.bz2 
```

### Historical Data Bootstrap

```text
peer-stats 0.1.0
peer-stats is a CLI tool that collects peer information from a given RIB dump file

USAGE:
    peer-stats-bootstrap [FLAGS] --output-dir <output-dir> --ts-end <ts-end> --ts-start <ts-start>

FLAGS:
        --debug         whether to print debug
        --dry-run       whether to dry run the code
    -h, --help          Prints help information
        --only-daily    whether to do only daily parsing
    -V, --version       Prints version information

OPTIONS:
        --output-dir <output-dir>    Output directory
        --ts-end <ts-end>            end timestamp
        --ts-start <ts-start>        start timestamp
```

## Output

The commandline tool, `peer-stats`, outputs JSON-formatted string to stdout. Debug messages are displayed to
stderr, and thus will not interrupt the result output.

The format is as follows (removed all peers info but one for conciseness):
```json
{
  "collector": "route-views.sg",
  "peers": {
    "2001:de8:4::13:6168:1": {
      "asn": 136168,
      "ip": "2001:de8:4::13:6168:1",
      "num_connected_asns": 4,
      "num_v4_pfxs": 0,
      "num_v6_pfxs": 40
    },
    "project": "route-views",
    "rib_dump_url": "http://archive.routeviews.org/route-views.sg/bgpdata/2022.02/RIBS/rib.20220205.1800.bz2"
  }
}
```

## LICENSE

This work is under [MIT](LICENSE) license.