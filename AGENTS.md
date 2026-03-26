# Agentic Coding Guidelines for peer-stats

This document provides guidelines for AI agents working on the peer-stats codebase.

## Build, Test, and Lint Commands

### Essential Commands
```bash
# Format code (run after any code changes)
cargo fmt

# Build the project
cargo build

# Build with all features
cargo build --all-features

# Run all tests
cargo test

# Run a specific test
cargo test test_dedup          # by function name
cargo test tests::test_dedup   # full path

# Run clippy (must pass with zero warnings)
cargo clippy --all-features -- -D warnings

# Full verification (run before committing)
cargo fmt && cargo build && cargo test && cargo clippy --all-features -- -D warnings
```

### Binaries
The project includes 5 binaries defined in `Cargo.toml`:
- `peer-stats-single-file` - single file processor
- `peer-stats-bootstrap` - bootstrap operation
- `peer-stats-index` - index peer stats
- `as2rel-index` - index AS relationships
- `pfx2as-index` - index prefix-to-AS mappings

Build specific binary: `cargo build --bin peer-stats-single-file`

## Code Style Guidelines

### General Principles
- Keep modules focused on single concerns
- Use processor pattern: `new()` → `process_*()` → `into_*()`
- Prefer composition over inheritance
- Keep public API surface minimal - don't expose internal implementation details

### Formatting
- Use `cargo fmt` to auto-format code
- Maximum line length: standard Rust (100 chars soft limit)
- Use 4 spaces for indentation
- Group imports: stdlib, external crates, then internal modules

### Imports
Order imports as follows:
1. Standard library imports
2. External crate imports (alphabetically)
3. Internal crate imports

Example:
```rust
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;

use ipnet::IpNet;
use serde::{Deserialize, Serialize};

use crate::types::SomeType;
```

### Naming Conventions
- **Structs**: PascalCase (e.g., `PeerStatsProcessor`, `As2RelCount`)
- **Functions/Methods**: snake_case (e.g., `process_path`, `into_peer_info`)
- **Constants**: SCREAMING_SNAKE_CASE (e.g., `TIER1`, `TIER1_V4`)
- **Variables**: snake_case (e.g., `peer_ip`, `as_path`)
- **Type parameters**: PascalCase, single letters preferred

### Types
- Use explicit types for public API
- Prefer `u32` for ASN values
- Use `usize` for counts and indices
- Use `Option<T>` for nullable values
- Use `Result<T, E>` for fallible operations (prefer `anyhow::Result`)

### Error Handling
- Use `anyhow::Result` for most operations
- Use `?` operator for error propagation
- Avoid unwrap/expect in production code; use proper error handling
- Add context to errors when crossing module boundaries

### Module Structure
Each processor module should contain:
1. Types (structs with derives)
2. Processor struct with:
   - `new()` constructor
   - `process_*()` methods for accumulating data
   - `into_*()` method for finalizing results
   - `Default` implementation

Example structure:
```rust
// 1. Public types
#[derive(Debug, Clone, Serialize)]
pub struct SomeInfo { ... }

// 2. Processor struct
pub struct SomeProcessor { ... }

impl SomeProcessor {
    pub fn new() -> Self { ... }
    pub fn process_element(&mut self, ...) { ... }
    pub fn into_results(self, ...) -> SomeInfo { ... }
}

impl Default for SomeProcessor {
    fn default() -> Self { Self::new() }
}
```

### Documentation
- Add doc comments (`///`) for all public items
- Document struct fields with `///`
- Use proper grammar in documentation
- Keep doc comments factual and concise

### Serializing/Deserializing
Use derives in this order: `#[derive(Debug, Clone, Serialize)]` or `#[derive(Debug, Clone, Serialize, Deserialize)]`

### Testing
- Place tests in `#[cfg(test)]` module at end of file
- Use descriptive test names: `test_dedup`, `test_read_rib`
- Include edge cases (empty collections, single items)
- Use `tracing` for logging in tests if needed

### Constants
- Define at module level, before types
- Use arrays with explicit sizes: `[u32; 16]` not `[u32]`
- Document what the constants represent

### Version Control
- Don't commit test output files (*.json)
- Run full verification before committing
- Keep commits atomic (one logical change per commit)
- Use present tense in commit messages (e.g., "Add feature", not "Added feature")

### Dependencies
Key dependencies to know:
- `bgpkit-parser` - BGP/MRT parsing
- `serde` / `serde_json` - Serialization
- `anyhow` - Error handling
- `ipnet` - IP network types
- `clap` - CLI argument parsing
- `rusqlite` - SQLite database
- `tracing` - Logging

### CI/CD Compliance
The GitHub Actions workflow (`.github/workflows/build.yml`) runs:
1. `cargo build --verbose`
2. `cargo clippy --all-features -- -D warnings`

All PRs must pass these checks with zero warnings.

## Project Structure
```
src/
├── lib.rs           # Main library with orchestration logic
├── as2rel.rs        # AS relationship processing
├── peer_stats.rs    # Peer statistics processing
├── pfx2as.rs        # Prefix-to-AS mapping
└── bin/             # CLI binaries
    ├── single-file.rs
    ├── bootstrap.rs
    ├── index-peer-stats.rs
    ├── index-as2rel.rs
    └── index-pfx2as.rs
```

## Common Patterns

### Processor Pattern
```rust
let mut processor = SomeProcessor::new();
for item in items {
    processor.process_element(item);
}
let result = processor.into_results(...);
```

### Error Propagation
```rust
use anyhow::Result;

pub fn some_function() -> Result<Output> {
    let data = fetch_data()?;
    process(data)?;
    Ok(output)
}
```

### Match with Options
```rust
match some_option {
    None => {}
    Some(value) => { /* process */ }
}
```
