# rssbot [![Build Status](https://github.com/iovxw/rssbot/workflows/Rust/badge.svg)](https://github.com/iovxw/rssbot/actions?query=workflow%3ARust) [![Github All Releases](https://img.shields.io/github/downloads/iovxw/rssbot/total.svg)](https://github.com/iovxw/rssbot/releases)

**Other Languages:** [Chinese](README.md)

Telegram RSS bot [@RustRssBot](http://t.me/RustRssBot)

**Supports:**
 - [x] RSS 0.9
 - [x] RSS 0.91
 - [x] RSS 0.92
 - [x] RSS 0.93
 - [x] RSS 0.94
 - [x] RSS 1.0
 - [x] RSS 2.0
 - [x] Atom 0.3
 - [x] Atom 1.0
 - [x] JSON Feed 1

## Usage

    /rss       - Display a list of currently subscribed RSS feeds
    /sub       - Subscribe to an RSS: /sub http://example.com/feed.xml
    /unsub     - Unsubscribe from an RSS: /unsub http://example.com/feed.xml
    /export    - Export to OPML

## Download

The pre-compiled binaries can be downloaded directly from [Releases](https://github.com/iovxw/rssbot/releases). Make sure to use the english binary (`rssbot-en-amd64-linux`). The Linux version is statically linked to *musl*, no other dependencies required.

## Compile

**Please try to download from the Link above, if that's not feasible or you have other requirements you should compile manually**

Install *Rust Nightly* and *Cargo* ([`rustup` recommended](https://www.rustup.rs/)) first, then:

```
LOCALE=en cargo build --release
```

The compiled files are available at: `./target/release/rssbot`

## Run

```
A simple Telegram RSS bot.

USAGE:
    rssbot [FLAGS] [OPTIONS] <token>

FLAGS:
    -h, --help          Prints help information
        --insecure      DANGER: Insecure mode, accept invalid TLS certificates
        --restricted    Make bot commands only accessible for group admins
    -V, --version       Prints version information

OPTIONS:
        --admin <user id>...        Private mode, only specified user can use this bot. This argument can be passed
                                    multiple times to allow multiple admins
        --api-uri <tgapi-uri>       Custom telegram api URI [default: https://api.telegram.org/]
    -d, --database <path>           Path to database [default: ./rssbot.json]
        --import-opml <opml-path>   Offline: import feeds from an OPML file into one subscriber, then exit
                                    (requires --subscriber; no bot token needed)
        --max-feed-size <bytes>     Maximum feed size, 0 is unlimited [default: 2097152]
        --max-interval <seconds>    Maximum fetch interval [default: 43200]
        --min-interval <seconds>    Minimum fetch interval [default: 300]
        --subscriber <user id>      Subscriber to merge the imported feeds into (with --import-opml)

ARGS:
    <token>    Telegram bot token [env: RSSBOT_TOKEN] (not required for offline commands)

NOTE: You can get <user id> using bots like @userinfobot @getidsbot
```

Please read the [official docs](https://core.telegram.org/bots#3-how-do-i-create-a-bot) to create a token.

## Offline OPML import

To bulk-migrate an OPML export (e.g. from Pocket or Feedly) without starting
the bot, use the offline `--import-opml` mode. It never accesses a feed or
the Telegram API and does not need a bot token:

```bash
rssbot -d rssbot.json --import-opml feeds.opml --subscriber 123456789
```

Behavior:

- Nested `<outline>` groups are walked recursively; only leaves with an
  `http://`/`https://` `xmlUrl` are imported. Other schemes (`ftp:`,
  `file:`, …), malformed URLs and group outlines are reported as
  diagnostics on stderr instead of aborting the batch.
- Feeds are deduplicated by their normalized URL (host case, default ports
  and surrounding whitespace are folded together); the first title wins.
- XML entities in titles and URLs are decoded into UTF-8; values are
  re-escaped correctly when they are written back out.
- The feeds are *merged* into the given subscriber: its existing
  subscriptions and every other subscriber's data are preserved, and feeds
  already present in the database simply gain one more subscriber.
- The whole import ends in a single atomic replacement of the JSON
  database. A parse error or a failed write leaves the previous database
  (and the in-memory state) untouched, so an import is all-or-nothing.
- Imported feeds have no TTL or item history: they follow the normal
  `--min-interval`/`--max-interval` scheduling once the bot runs, and their
  current items are delivered on the first online fetch (the bot cannot
  inspect a feed while offline).
- Re-running the same import is idempotent and reports zero new
  subscriptions.


## Environment variables

- `RSSBOT_TOKEN`: Telegram bot token
- `HTTP_PROXY`: Proxy for HTTP
- `HTTPS_PROXY`: Proxy for HTTPS
- `RSSBOT_DONT_PROXY_FEEDS`: Set to `1` to limit the proxy to Telegram requests
- `NO_PROXY`: Not supported yet, wait for [reqwest#877](https://github.com/seanmonstar/reqwest/pull/877)

## Migrating from the old RSSBot

For the [original version of Clojure Bot ](https://github.com/iovxw/tg-rss-bot), you can use the following script to convert the database:

```bash
#!/bin/bash

DATABASE=$1
TARGET=$2

DATA=$(echo "SELECT url, title FROM rss;" | sqlite3 $DATABASE)
IFS=$'\n'

echo -e "[\c" > $TARGET
for line in ${DATA[@]}
do
    IFS='|'
    r=($line)
    link=${r[0]}
    title=${r[1]}

    echo -e "{\"link\":\"$link\"," \
            "\"title\":\"$title\"," \
            "\"error_count\":0," \
            "\"hash_list\":[]," \
            "\"subscribers\":[\c" >> $TARGET

    subscribers=$(echo "SELECT subscriber FROM subscribers WHERE rss='$link';" | sqlite3 $DATABASE)
    IFS=$'\n'
    for subscriber in ${subscribers[@]}
    do
        echo -e "$subscriber,\c" >> $TARGET
    done

    echo -e "]},\c" >> $TARGET
done
echo "]" >> $TARGET
sed -i "s/,]/]/g" $TARGET
```

Parameter 1 is the old database path, parameter 2 is the resulting output JSON path.

It should be noted that the RSS records that have been pushed will not be marked. If the converted database is used directly, the old RSS will be pushed repeatedly when the script is called again.

## License

This is free and unencumbered software released into the public domain.

Anyone is free to copy, modify, publish, use, compile, sell, or distribute this software, either in source code form or as a compiled binary, for any purpose, commercial or non-commercial, and by any means.
