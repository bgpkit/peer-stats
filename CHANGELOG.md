# Changelog

All notable changes to this project will be documented in this file.

## Unreleased

### Code Refactoring

* Refactored lib.rs into dedicated modules (as2rel, peer_stats, pfx2as) with processor pattern
* Moved types and constants into their corresponding processor modules
* Removed unnecessary internal function exports from public API

### Bug Fixes

* Removed AS 1239 (Sprint) from tier-1 ASN list to match bgp.tools definition
* Removed unnecessary ASN 0 placeholder from TIER1_V4 array

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