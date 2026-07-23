# Changelog

All notable changes to this project will be documented in this file.

## Unreleased

### New Features

* Add collector-aware AS relationship output alongside classic aggregate (#13)
  * `as2rel-index` now produces per-collector provenance files (`*-collector-latest.json.bz2`)
  * Classic output unchanged — no breaking changes to existing data pipeline
  * Single file pass produces both outputs for all three prefixes (as2rel, as2rel-v4, as2rel-v6)
  * Memory-efficient flat-vector + sort approach (~1.7GB peak RSS for 21M records)

### Performance

* Switch to route-level BGP parser (`into_route_iter`) from bgpkit-parser v0.18.0
  * ~19% faster on RIPE RIS bview files; larger gains expected on full RIB dumps
  * Eliminates per-path allocation by borrowing AS path via `Arc`

### Dependencies

* bgpkit-parser: 0.11.0 → 0.18.0 (route-level parser, AS4_PATH merge fix, new attribute parsers)
* bgpkit-broker: 0.7.5 → 0.11.0 (new collectors: locix.fra, ixpn.lagos, decix.fra, crix.sjo; SDK caching)

### Code Refactoring

* Refactored lib.rs into dedicated modules (as2rel, peer_stats, pfx2as) with processor pattern
* Moved types and constants into their corresponding processor modules
* Removed unnecessary internal function exports from public API

### Algorithm Changes

* Added two-tier transit detection: `TRUE_TIER1` (14 ASes always valid) vs candidate tier-1 ASes (Zayo, Hurricane Electric) that are only valid transit providers when their next hop is also a tier-1
* Hurricane Electric (AS 6939) is treated as a candidate tier-1 for IPv6 only, reducing its downstream count by ~70% at tested collectors
* Zayo (AS 6461) is treated as a candidate tier-1 for both IPv4 and IPv6, reducing its downstream count by 14-18%

### Bug Fixes

* Removed AS 1239 (Sprint) from tier-1 ASN list to match bgp.tools definition
* Removed unnecessary ASN 0 placeholder from TIER1_V4 array
* Fix candidate tier-1 transit detection to stop at failing candidates (#12)
* Fix clippy warnings in bootstrap.rs (useless borrows in format! macros)

## v0.2.1 - 2025-04-09

### Highlights

* make sure we don't use more threads than the system has available
* update dependencies

## v0.2.0 - 2024-02-01

### Highlights

* fix v4/v6 as2rel issue by @digizeph in https://github.com/bgpkit/peer-stats/pull/8

## v0.1.1 - 2023-11-29

### What's Changed

* add new command to index information into sqlite database by @digizeph in https://github.com/bgpkit/peer-stats/pull/2
* Update dependencies by @digizeph in https://github.com/bgpkit/peer-stats/pull/5
* Env vars by @digizeph in https://github.com/bgpkit/peer-stats/pull/6
