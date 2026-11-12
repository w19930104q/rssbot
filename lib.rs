//! Test-only library facade: the production artifact is the `rssbot`
//! binary; this crate exists so integration tests (`tests/`) can reach the
//! internal modules. It is compiled for `cargo test` but is never installed.

#[path = "src/data.rs"]
pub mod data;
#[path = "src/feed.rs"]
pub mod feed;
#[path = "src/opml.rs"]
pub mod opml;
