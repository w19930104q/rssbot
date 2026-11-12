# rssbot [![Build Status](https://github.com/iovxw/rssbot/workflows/Rust/badge.svg)](https://github.com/iovxw/rssbot/actions?query=workflow%3ARust) [![Github All Releases](https://img.shields.io/github/downloads/iovxw/rssbot/total.svg)](https://github.com/iovxw/rssbot/releases)

**Other Languages:** [English](README.en.md)

Telegram RSS 机器人 [@RustRssBot](http://t.me/RustRssBot)

**支持:**
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

## 使用

    /rss       - 显示当前订阅的 RSS 列表
    /sub       - 订阅一个 RSS: /sub http://example.com/feed.xml
    /unsub     - 退订一个 RSS: /unsub http://example.com/feed.xml
    /export    - 导出为 OPML

## 下载

可直接从 [Releases](https://github.com/iovxw/rssbot/releases) 下载预编译的程序（带 `zh` 的为中文版）, Linux 版本为 *musl* 静态链接, 无需其他依赖

## 编译

**请先尝试从上面下载, 如不可行或者有其他需求再手动编译**

先安装 *Rust Nightly* 以及 *Cargo* (推荐使用 [`rustup`](https://www.rustup.rs/)), 然后:

```
cargo build --release
```

编译好的文件位于: `./target/release/rssbot`

## 运行

```
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
        --import-opml <opml-path>   离线模式：把 OPML 文件中的订阅导入指定 subscriber 后退出
                                    （需配合 --subscriber，无需 bot token）
        --max-feed-size <bytes>     Maximum feed size, 0 is unlimited [default: 2097152]
        --max-interval <seconds>    Maximum fetch interval [default: 43200]
        --min-interval <seconds>    Minimum fetch interval [default: 300]
        --subscriber <user id>      导入订阅合并到的 subscriber（配合 --import-opml 使用）

ARGS:
    <token>    Telegram bot token [env: RSSBOT_TOKEN]（离线命令不需要）

NOTE: You can get <user id> using bots like @userinfobot @getidsbot
```

`<token>` 请参照 [这里](https://core.telegram.org/bots#3-how-do-i-create-a-bot) 申请

## 离线导入 OPML

需要批量迁移 Pocket/Feedly 等导出的 OPML 时，可以使用离线的 `--import-opml`
模式，无需启动机器人、不需要 bot token，也不会访问任何 feed 或 Telegram API：

```bash
rssbot -d rssbot.json --import-opml feeds.opml --subscriber 123456789
```

行为说明：

- 递归遍历嵌套的 `<outline>` 分组，只导入 `xmlUrl` 为 `http://`/`https://`
  的叶子节点；其他协议（`ftp:`、`file:` 等）、非法 URL 和纯分组节点会作为
  诊断信息输出到 stderr，而不会让整批导入失败。
- 按规范化后的 URL 去重（主机名大小写、默认端口、首尾空白会被归一化），
  标题保留第一次出现的版本。
- 标题和 URL 中的 XML 实体会正确解码为 UTF-8；写回时也会正确再转义。
- 导入是*合并*语义：该 subscriber 已有的订阅、以及其他 subscriber 的数据
  都会保留；数据库中已存在的 feed 只会增加一个订阅者。
- 整个导入最终通过一次原子替换写入 JSON 数据库。解析失败或写入失败时，
  旧数据库（以及内存状态）保持不变，导入要么全部成功要么完全不生效。
- 导入的 feed 没有 TTL 和条目历史：机器人运行后按正常的
  `--min-interval`/`--max-interval` 调度，首次在线抓取会推送当前已有条目
  （离线状态下机器人无法读取 feed 内容）。
- 重复执行同一份导入是幂等的，会报告新增订阅数为 0。


## 环境变量

- `RSSBOT_TOKEN`: Telegram bot token
- `HTTP_PROXY`: 用于 HTTP 的代理
- `HTTPS_PROXY`: 用于 HTTPS 的代理
- `RSSBOT_DONT_PROXY_FEEDS`: 设为 `1` 使所有订阅的 RSS 不通过代理（仅代理 Telegram）
- `NO_PROXY`: 暂不支持，等待 [reqwest#877](https://github.com/seanmonstar/reqwest/pull/877)

## 从旧的 RSSBot 迁移

对于 [原先 Clojure 版本的 Bot](https://github.com/iovxw/tg-rss-bot), 可以使用以下脚本转换数据库

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

参数 1 为旧数据库地址, 2 为结果输出地址

需要注意的是已推送的 RSS 记录不会保留, 如果直接使用转换后的数据库, 会重复推送旧的 RSS

## License

This is free and unencumbered software released into the public domain.

Anyone is free to copy, modify, publish, use, compile, sell, or distribute this software, either in source code form or as a compiled binary, for any purpose, commercial or non-commercial, and by any means.
