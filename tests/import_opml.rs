//! End-to-end regression tests for the offline `--import-opml` feature.
//!
//! These tests exercise the real parse -> merge -> atomic JSON write path
//! through the public-ish test facade (`lib.rs`) and on-disk database files.
//! Nothing in here performs network or Telegram access.

use std::fs;
use std::path::PathBuf;

use rssbot::data::{Database, Feed};
use rssbot::opml;

const NESTED_OPML: &str = include_str!("data/import_nested.opml");
const MIXED_OPML: &str = include_str!("data/import_mixed.opml");

struct TempDb {
    path: PathBuf,
}

impl TempDb {
    fn new() -> Self {
        // Unique per test so parallel test execution never collides.
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rssbot-import-{pid}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        TempDb {
            path: dir.join("rssbot.json"),
        }
    }

    fn open(&self) -> Database {
        Database::open(self.path.clone()).unwrap()
    }

    /// Read back the persisted JSON database as a list of feeds.
    fn read_feeds(&self) -> Vec<Feed> {
        let raw = fs::read(&self.path).unwrap();
        serde_json::from_slice(&raw).unwrap()
    }

    fn write_raw(&self, contents: &str) {
        fs::write(&self.path, contents).unwrap();
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        if let Some(dir) = self.path.parent() {
            let _ = fs::remove_dir_all(dir);
        }
    }
}

#[test]
fn imports_nested_groups_and_persists_utf8_titles() {
    let tmp = TempDb::new();
    let (feeds, diagnostics) =
        opml::parse_opml(std::io::Cursor::new(NESTED_OPML.as_bytes())).unwrap();

    // All five leaves across three nesting levels are found.
    assert_eq!(feeds.len(), 5);
    assert!(diagnostics.bad_urls.is_empty());
    assert_eq!(diagnostics.duplicates, 0);

    let mut db = tmp.open();
    let added = db.import_feeds(42, &feeds).unwrap();
    assert_eq!(added, 5);
    drop(db);

    let stored = tmp.read_feeds();
    assert_eq!(stored.len(), 5);

    // XML entities were decoded into real UTF-8 text, not left escaped.
    let cn = stored
        .iter()
        .find(|f| f.link == "https://cn.example.com/rss?lang=zh&fmt=rss")
        .expect("utf-8 feed persisted");
    assert_eq!(cn.title, "中文资讯 <测试>");

    let blog = stored
        .iter()
        .find(|f| f.link == "https://blog.rust-lang.org/feed.xml")
        .unwrap();
    assert_eq!(blog.title, "Rust Blog & Friends");

    // Imported feeds start without fetch state: TTL semantics are untouched
    // and the gardener/fetcher will populate these lazily.
    assert!(stored
        .iter()
        .all(|f| f.ttl.is_none() && f.down_time.is_none()));

    // Reopening rebuilds the subscriber index correctly.
    let reopened = tmp.open();
    assert!(reopened.is_subscribed(42, "https://news.ycombinator.com/rss"));
    assert_eq!(reopened.subscribed_feeds(42).unwrap().len(), 5);
}

#[test]
fn bad_urls_are_diagnosed_but_good_feeds_still_import() {
    let tmp = TempDb::new();
    let (feeds, diagnostics) =
        opml::parse_opml(std::io::Cursor::new(MIXED_OPML.as_bytes())).unwrap();

    // Only the two http(s) leaves survive: the duplicated spelling, the
    // ftp/file/garbage/empty xmlUrls are all rejected.
    assert_eq!(feeds.len(), 2);
    assert_eq!(
        diagnostics.bad_urls,
        vec![
            "ftp://files.example.com/dump".to_owned(),
            "not a url".to_owned(),
            "file:///etc/passwd".to_owned(),
        ]
    );
    assert_eq!(diagnostics.duplicates, 1);

    let mut db = tmp.open();
    let added = db.import_feeds(7, &feeds).unwrap();
    assert_eq!(added, 2);

    let links: Vec<String> = tmp.read_feeds().into_iter().map(|f| f.link).collect();
    assert!(links.contains(&"https://dup.example.com/feed".to_owned()));
    assert!(links.contains(&"https://nested.example.com/rss".to_owned()));
    assert!(!links.iter().any(|l| l.starts_with("ftp://")));
}

#[test]
fn merge_keeps_existing_subscriptions_and_other_subscribers_intact() {
    let tmp = TempDb::new();

    // Seed the database: subscriber 1 owns an "old" feed and shares a
    // "shared" feed with subscriber 2.
    let seed = r#"[
      {
        "link": "https://old.example.com/rss",
        "title": "Pre-existing",
        "down_time": null,
        "subscribers": [1],
        "ttl": 900,
        "hash_list": [12345]
      },
      {
        "link": "https://shared.example.com/rss",
        "title": "Shared feed",
        "down_time": null,
        "subscribers": [1, 2],
        "ttl": null,
        "hash_list": []
      }
    ]"#;
    tmp.write_raw(seed);

    let opml = r#"<?xml version="1.0"?>
    <opml><body>
      <outline type="rss" text="New one" xmlUrl="https://new.example.com/feed"/>
      <outline type="rss" text="Already mine" xmlUrl="https://old.example.com/rss"/>
      <outline type="rss" text="Shared with sub 2" xmlUrl="https://shared.example.com/rss"/>
    </body></opml>"#;
    let (feeds, _) = opml::parse_opml(std::io::Cursor::new(opml.as_bytes())).unwrap();

    let mut db = tmp.open();
    let added = db.import_feeds(1, &feeds).unwrap();
    // Only the genuinely new feed counts; the other two were already
    // subscribed by subscriber 1.
    assert_eq!(added, 1);
    drop(db);

    let stored = tmp.read_feeds();
    assert_eq!(stored.len(), 3);

    let shared = stored
        .iter()
        .find(|f| f.link == "https://shared.example.com/rss")
        .unwrap();
    assert!(shared.subscribers.contains(&1));
    assert!(
        shared.subscribers.contains(&2),
        "other subscriber preserved"
    );

    let old = stored
        .iter()
        .find(|f| f.link == "https://old.example.com/rss")
        .unwrap();
    // Existing feed metadata is never overwritten by the import.
    assert_eq!(old.title, "Pre-existing");
    assert_eq!(old.ttl, Some(900));
    assert_eq!(old.hash_list, vec![12345]);

    let reopened = tmp.open();
    assert_eq!(reopened.subscribed_feeds(1).unwrap().len(), 3);
    assert_eq!(reopened.subscribed_feeds(2).unwrap().len(), 1);
}

#[test]
fn re_importing_the_same_opml_is_idempotent() {
    let tmp = TempDb::new();
    let (feeds, _) = opml::parse_opml(std::io::Cursor::new(NESTED_OPML.as_bytes())).unwrap();

    let mut db = tmp.open();
    assert_eq!(db.import_feeds(99, &feeds).unwrap(), 5);
    // Second run: nothing new, and the file must not be rewritten.
    let mtime_before = fs::metadata(&tmp.path).unwrap().modified().unwrap();
    assert_eq!(db.import_feeds(99, &feeds).unwrap(), 0);
    let mtime_after = fs::metadata(&tmp.path).unwrap().modified().unwrap();
    assert_eq!(mtime_before, mtime_after);
    assert_eq!(tmp.read_feeds().len(), 5);
}

#[test]
fn malformed_opml_does_not_touch_a_missing_database() {
    let tmp = TempDb::new();
    assert!(!tmp.path.exists());

    let broken = "<opml><body><outline xmlUrl=\"https://x.example/\"></body>";
    let result = opml::parse_opml(std::io::Cursor::new(broken.as_bytes()));
    assert!(result.is_err());

    // The database is opened only after parsing succeeds, so no file and no
    // directory content was created for the failed parse.
    assert!(!tmp.path.exists());
}

#[test]
fn write_failure_rolls_back_in_memory_state_and_keeps_old_file() {
    // Point the database at a path inside a read-only directory so the
    // atomic rename cannot succeed.
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("rssbot-rollback-{pid}-{nanos}"));
    fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join("rssbot.json");

    // Seed a valid existing database; its bytes must survive the failed save.
    let seed = r#"[{
        "link": "https://keep.example.com/rss",
        "title": "Keep me",
        "down_time": null,
        "subscribers": [3],
        "ttl": null,
        "hash_list": []
    }]"#;
    fs::write(&db_path, seed).unwrap();
    let before = fs::read(&db_path).unwrap();

    // Make the directory read-only (best effort; root would ignore this,
    // but the sandbox runs as a normal user).
    let mut perms = fs::metadata(&dir).unwrap().permissions();
    use std::os::unix::fs::PermissionsExt;
    perms.set_mode(0o500);
    fs::set_permissions(&dir, perms).unwrap();

    let opml = r#"<?xml version="1.0"?>
    <opml><body>
      <outline type="rss" text="Should not persist" xmlUrl="https://lost.example.com/feed"/>
    </body></opml>"#;
    let (feeds, _) = opml::parse_opml(std::io::Cursor::new(opml.as_bytes())).unwrap();

    let mut db = Database::open(db_path.clone()).unwrap();
    let result = db.import_feeds(3, &feeds);
    assert!(result.is_err(), "write must fail in a read-only directory");

    // In-memory state was rolled back to the old subscriptions.
    assert!(db.is_subscribed(3, "https://keep.example.com/rss"));
    assert!(!db.is_subscribed(3, "https://lost.example.com/feed"));
    assert_eq!(db.subscribed_feeds(3).unwrap().len(), 1);
    drop(db);

    // Restore permissions so TempDir-style cleanup can run, and verify the
    // on-disk file is byte-for-byte the previous database.
    let mut perms = fs::metadata(&dir).unwrap().permissions();
    perms.set_mode(0o700);
    fs::set_permissions(&dir, perms).unwrap();
    let after = fs::read(&db_path).unwrap();
    assert_eq!(before, after);

    let reopened = Database::open(db_path).unwrap();
    assert_eq!(reopened.all_feeds().len(), 1);
    assert!(!reopened.is_subscribed(3, "https://lost.example.com/feed"));

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn gbk_encoded_opml_is_decoded_to_utf8() {
    // quick-xml's `encoding` feature honors the XML declaration; a GBK file
    // must yield the same decoded UTF-8 title/URL as a UTF-8 one.
    let bytes = include_bytes!("data/import_gbk.opml");
    let (feeds, diagnostics) =
        opml::parse_opml(std::io::Cursor::new(bytes.as_slice())).unwrap();
    assert!(diagnostics.bad_urls.is_empty());
    assert_eq!(feeds.len(), 1);
    assert_eq!(feeds[0].title, "中文 GBK 标题 & 测试");
    assert_eq!(feeds[0].link, "https://gbk.example.com/feed?a=1&b=2");
}

#[test]
fn opml_export_roundtrip_preserves_escaped_entities() {
    // A feed whose title/URL contain characters that need XML escaping must
    // survive export -> re-import with identical decoded values.
    let tmp = TempDb::new();
    let mut feed = Feed::default();
    feed.link = "https://roundtrip.example.com/feed?a=1&b=2".to_owned();
    feed.title = "Title with <xml> & \"quotes\" 中文".to_owned();
    feed.subscribers.insert(11);

    let mut db = tmp.open();
    let added = db
        .import_feeds(
            11,
            &[opml::OpmlFeed {
                title: feed.title.clone(),
                link: feed.link.clone(),
            }],
        )
        .unwrap();
    assert_eq!(added, 1);

    let exported = opml::into_opml(vec![feed.clone()]);
    let (reparsed, diagnostics) =
        opml::parse_opml(std::io::Cursor::new(exported.as_bytes())).unwrap();
    assert!(diagnostics.bad_urls.is_empty());
    assert_eq!(reparsed.len(), 1);
    assert_eq!(reparsed[0].title, feed.title);
    assert_eq!(reparsed[0].link, feed.link);
}
