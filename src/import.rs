use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use anyhow::Context;

use crate::data::{Database, SubscriberId};
use crate::opml;

/// Execute the offline `--import-opml` command.
///
/// The OPML file is parsed *before* the database is touched, so a malformed
/// document never creates or modifies the JSON database. Parsing only reads
/// the local file: no feed URL is fetched and no Telegram API is called.
///
/// Returns the process exit code. Any merge is committed through
/// [`Database::import_feeds`], which writes the JSON database with a single
/// atomic rename and rolls back in-memory state on failure, meaning parse or
/// write failures leave the previous database state untouched.
pub fn run_import(
    database_path: &Path,
    opml_path: &Path,
    subscriber: SubscriberId,
) -> anyhow::Result<i32> {
    // 1. Parse and validate the whole OPML document first.
    let opml_file = File::open(opml_path)
        .with_context(|| format!("Failed to open OPML file: {}", opml_path.display()))?;
    let (feeds, diagnostics) = opml::parse_opml(BufReader::new(opml_file))
        .with_context(|| format!("Failed to parse OPML file: {}", opml_path.display()))?;

    // 2. Open (or create) the database only after parsing succeeded.
    let mut database = Database::open(database_path.to_path_buf())
        .with_context(|| format!("Failed to open database: {}", database_path.display()))?;

    // 3. Merge into the subscriber; one atomic JSON replacement on success.
    let added = database
        .import_feeds(subscriber, &feeds)
        .context("Failed to write database; existing state was kept")?;

    report(&diagnostics, feeds.len(), added);
    Ok(0)
}

fn report(diagnostics: &opml::OpmlDiagnostics, parsed: usize, added: usize) {
    println!(
        "Import finished: {} feed(s) in OPML, {} new subscription(s) added for subscriber",
        parsed, added
    );
    if diagnostics.duplicates > 0 {
        println!(
            "Skipped {} duplicate entr(ies) already present in the OPML",
            diagnostics.duplicates
        );
    }
    if !diagnostics.bad_urls.is_empty() {
        // Bad URLs are recorded as diagnostics, not a batch failure: the
        // process still exits 0 after importing everything usable.
        eprintln!(
            "Skipped {} outline(s) with missing, malformed or non-http(s) xmlUrl:",
            diagnostics.bad_urls.len()
        );
        for url in &diagnostics.bad_urls {
            eprintln!("  - {url}");
        }
    }
    if parsed == 0 && diagnostics.bad_urls.is_empty() {
        // An empty but valid OPML: not an error, just make it visible.
        println!("OPML contained no usable feed outlines; database unchanged");
    }
}
