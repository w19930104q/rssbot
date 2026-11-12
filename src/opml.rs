use std::io::Cursor;
use std::io::Write;

use chrono::Local;
use quick_xml::events::attributes::Attribute;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use quick_xml::{Reader, Writer};
use url::Url;

use crate::data::Feed;

pub fn into_opml(feeds: Vec<Feed>) -> String {
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    let decl = BytesDecl::new(b"1.0", Some(b"UTF-8"), None);
    writer.write_event(Event::Decl(decl)).unwrap();

    with_tag(
        &mut writer,
        b"opml",
        &mut [Attribute::from(("version", "2.0")).into()],
        |writer| {
            with_tag(writer, b"head", &mut [], |writer| {
                with_tag(writer, b"title", &mut [], |writer| {
                    let text = BytesText::from_plain(b"Exported from RSSBot");
                    writer.write_event(Event::Text(text))?;
                    Ok(())
                })?;
                with_tag(writer, b"dateCreated", &mut [], |writer| {
                    // e.g. Thu, 02 Nov 2017 18:08:24 CST
                    let time = Local::now().format("%a, %d %b %Y %T %Z").to_string();
                    let text = BytesText::from_plain_str(&time);
                    writer.write_event(Event::Text(text))?;
                    Ok(())
                })?;
                with_tag(writer, b"docs", &mut [], |writer| {
                    let text = BytesText::from_plain(b"http://www.opml.org/spec2");
                    writer.write_event(Event::Text(text))?;
                    Ok(())
                })
            })?;
            with_tag(writer, b"body", &mut [], move |writer| {
                for feed in feeds {
                    let mut outline = BytesStart::borrowed(b"outline", 7);
                    outline.push_attribute(Attribute::from(("type", "rss")));
                    outline.push_attribute(Attribute::from(("text", feed.title.as_str())));
                    outline.push_attribute(Attribute::from(("xmlUrl", feed.link.as_str())));
                    writer.write_event(Event::Empty(outline))?;
                }
                Ok(())
            })
        },
    )
    .unwrap();

    unsafe { String::from_utf8_unchecked(writer.into_inner().into_inner()) }
}

// type of `attrs` is for zero allocation
fn with_tag<W, F>(
    writer: &mut Writer<W>,
    tag: &[u8],
    attrs: &mut [Option<Attribute<'_>>],
    then: F,
) -> quick_xml::Result<()>
where
    W: Write,
    F: FnOnce(&mut Writer<W>) -> quick_xml::Result<()>,
{
    let mut start = BytesStart::borrowed(tag, tag.len());
    for attr in attrs.iter_mut() {
        start.push_attribute(attr.take().unwrap());
    }
    writer.write_event(Event::Start(start)).unwrap();
    then(writer)?;
    let end = BytesEnd::borrowed(tag);
    writer.write_event(Event::End(end)).unwrap();
    Ok(())
}

#[test]
fn test_to_opml() {
    let mut feed1 = Feed::default();
    feed1.title = "title1".into();
    feed1.link = "link1".into();
    let mut feed2 = Feed::default();
    feed2.title = "title2".into();
    feed2.link = "link2".into();
    let feeds = vec![feed1, feed2];
    let r = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <opml version=\"2.0\">\
         <head>\
         <title>Exported from RSSBot</title>\
         <dateCreated>{}</dateCreated>\
         <docs>http://www.opml.org/spec2</docs>\
         </head>\
         <body>\
         <outline type=\"rss\" text=\"title1\" xmlUrl=\"link1\"/>\
         <outline type=\"rss\" text=\"title2\" xmlUrl=\"link2\"/>\
         </body>\
         </opml>",
        Local::now().format("%a, %d %b %Y %T %Z")
    );
    assert_eq!(into_opml(feeds), r);
}

/// A single feed extracted from an OPML outline.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct OpmlFeed {
    /// Decoded title (`title` attribute, falling back to `text`)
    pub title: String,
    /// Normalized feed URL, always http(s)
    pub link: String,
}

/// Non-fatal problems collected while parsing an OPML document.
#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct OpmlDiagnostics {
    /// `xmlUrl` values that were missing, malformed or not http(s)
    pub bad_urls: Vec<String>,
    /// Entries dropped because their normalized URL was already seen
    pub duplicates: usize,
}

/// Validate and normalize a feed URL. Only http(s) URLs are accepted;
/// the URL is re-serialized (e.g. host lower-cased, default ports folded)
/// so equivalent spellings deduplicate to the same entry.
fn normalize_feed_url(raw: &str) -> Option<String> {
    let url = Url::parse(raw.trim()).ok()?;
    match url.scheme() {
        "http" | "https" => Some(url.as_str().to_owned()),
        _ => None,
    }
}

/// Parse an OPML document (as exported by Pocket/Feedly) into a
/// deduplicated list of feeds, recursively walking nested `<outline>` groups.
///
/// - Only `xmlUrl` values with an `http`/`https` scheme are accepted;
///   group outlines without a usable `xmlUrl` are transparent and bad URLs
///   are recorded in the diagnostics instead of failing the whole batch.
/// - XML entities (named, numeric and hex) in titles and URLs are decoded
///   into plain UTF-8 `String`s; re-escaping happens when writing OPML/JSON.
/// - Feeds are deduplicated by normalized URL, the first title wins.
/// - A malformed document returns an `Err`; the caller must leave any
///   existing state untouched.
pub fn parse_opml<R: std::io::BufRead>(
    reader: R,
) -> quick_xml::Result<(Vec<OpmlFeed>, OpmlDiagnostics)> {
    let mut reader = Reader::from_reader(reader);
    reader.trim_text(true);

    let mut buf = Vec::new();
    let mut feeds: Vec<OpmlFeed> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut diagnostics = OpmlDiagnostics::default();

    loop {
        match reader.read_event(&mut buf)? {
            // Both group outlines (`<outline>…</outline>`) and leaf outlines
            // (`<outline …/>`) arrive here; the flat event stream means nested
            // groups need no explicit recursion.
            Event::Start(ref e) | Event::Empty(ref e) if e.name() == b"outline" => {
                let mut title = None;
                let mut text = None;
                let mut xml_url = None;
                for attr in e.attributes() {
                    let attr = attr?;
                    match &*reader.decode(attr.key) {
                        "title" => title = Some(attr.unescape_and_decode_value(&reader)?),
                        "text" => text = Some(attr.unescape_and_decode_value(&reader)?),
                        "xmlUrl" => xml_url = Some(attr.unescape_and_decode_value(&reader)?),
                        _ => (),
                    }
                }

                match xml_url {
                    // Empty xmlUrl is just an unlabeled group outline.
                    Some(raw) if !raw.trim().is_empty() => match normalize_feed_url(&raw) {
                        Some(link) => {
                            if seen.insert(link.clone()) {
                                feeds.push(OpmlFeed {
                                    title: title.or_else(|| text).unwrap_or_default(),
                                    link,
                                });
                            } else {
                                diagnostics.duplicates += 1;
                            }
                        }
                        None => diagnostics.bad_urls.push(raw.trim().to_owned()),
                    },
                    _ => (),
                }
            }
            Event::Eof => break,
            _ => (),
        }
        buf.clear();
    }

    Ok((feeds, diagnostics))
}

#[cfg(test)]
mod import_test {
    use super::*;
    use std::io::Cursor;

    fn parse(xml: &str) -> (Vec<OpmlFeed>, OpmlDiagnostics) {
        parse_opml(Cursor::new(xml.as_bytes())).unwrap()
    }

    #[test]
    fn nested_groups_are_walked_recursively() {
        let xml = "\
<?xml version=\"1.0\" encoding=\"UTF-8\"?>
<opml version=\"2.0\"><head><title>x</title></head><body>
  <outline text=\"tech\">
    <outline text=\"rust\">
      <outline type=\"rss\" title=\"This Week in Rust\" xmlUrl=\"https://rust-weekly.example/feed\"/>
    </outline>
    <outline type=\"rss\" text=\"Other\" xmlUrl=\"http://other.example/rss\"/>
  </outline>
  <outline type=\"rss\" text=\"Top level\" xmlUrl=\"https://top.example/feed.xml\"/>
</body></opml>";
        let (feeds, diag) = parse(xml);
        assert!(diag.bad_urls.is_empty());
        assert_eq!(diag.duplicates, 0);
        assert_eq!(
            feeds.iter().map(|f| f.link.as_str()).collect::<Vec<_>>(),
            vec![
                "https://rust-weekly.example/feed",
                "http://other.example/rss",
                "https://top.example/feed.xml",
            ]
        );
        // `title` takes precedence over `text`
        assert_eq!(feeds[0].title, "This Week in Rust");
        // Falls back to `text` when there is no `title`
        assert_eq!(feeds[1].title, "Other");
        assert_eq!(feeds[2].title, "Top level");
    }

    #[test]
    fn duplicates_by_normalized_url_are_collapsed() {
        let xml = "\
<opml><body>
  <outline text=\"a\" xmlUrl=\"https://Example.com/feed\"/>
  <outline text=\"b\" xmlUrl=\"  https://example.com/feed  \"/>
  <outline text=\"c\" xmlUrl=\"https://example.com:443/feed?x=1&amp;y=2\"/>
  <outline text=\"d\" xmlUrl=\"HTTPS://EXAMPLE.COM/feed?x=1&amp;y=2\"/>
</body></opml>";
        let (feeds, diag) = parse(xml);
        // Host case + surrounding whitespace collapse to the same URL, and
        // the explicit default https port is folded away by normalization.
        assert_eq!(feeds.len(), 2);
        assert_eq!(feeds[0].link, "https://example.com/feed");
        assert_eq!(feeds[0].title, "a");
        assert_eq!(feeds[1].link, "https://example.com/feed?x=1&y=2");
        assert_eq!(diag.duplicates, 2);
        assert!(diag.bad_urls.is_empty());
    }

    #[test]
    fn bad_urls_are_diagnosed_without_aborting() {
        let xml = "\
<opml><body>
  <outline text=\"ftp\" xmlUrl=\"ftp://example.com/feed\"/>
  <outline text=\"broken\" xmlUrl=\"not a url at all\"/>
  <outline text=\"javascript\" xmlUrl=\"javascript:alert(1)\"/>
  <outline text=\"empty\" xmlUrl=\"   \"/>
  <outline text=\"group\"/>
  <outline text=\"good\" xmlUrl=\"https://good.example/rss\"/>
</body></opml>";
        let (feeds, diag) = parse(xml);
        assert_eq!(feeds.len(), 1);
        assert_eq!(feeds[0].link, "https://good.example/rss");
        assert_eq!(
            diag.bad_urls,
            vec![
                "ftp://example.com/feed".to_owned(),
                "not a url at all".to_owned(),
                "javascript:alert(1)".to_owned(),
            ]
        );
        // Empty xmlUrl and group outlines are silently skipped
        assert_eq!(diag.duplicates, 0);
    }

    #[test]
    fn utf8_titles_and_entities_are_decoded() {
        let xml = "\
<opml><body>
  <outline title=\"中文 &amp; 日本語 &lt;ok&gt; &#65;&quot;q&quot;\" xmlUrl=\"https://x.example/feed\"/>
  <outline title=\"hex entity &#x4e2d;\" xmlUrl=\"https://y.example/feed?a=1&amp;b=2\"/>
</body></opml>";
        let (feeds, _) = parse(xml);
        assert_eq!(feeds[0].title, "中文 & 日本語 <ok> A\"q\"");
        assert_eq!(feeds[1].title, "hex entity 中");
        // Entity in the URL is decoded too
        assert_eq!(feeds[1].link, "https://y.example/feed?a=1&b=2");
    }

    #[test]
    fn malformed_document_is_an_error() {
        let xml = "<opml><body><outline text=\"x\" xmlUrl=\"https://x.example/\"></body>";
        let result = parse_opml(Cursor::new(xml.as_bytes()));
        assert!(result.is_err());
    }

    #[test]
    fn normalized_url_accepts_only_http_schemes() {
        assert_eq!(
            normalize_feed_url("HTTPS://Example.com:443/a"),
            Some("https://example.com/a".to_owned())
        );
        assert_eq!(
            normalize_feed_url("http://example.com/b"),
            Some("http://example.com/b".to_owned())
        );
        assert_eq!(normalize_feed_url("feed://example.com/c"), None);
        assert_eq!(normalize_feed_url("not url"), None);
    }
}
