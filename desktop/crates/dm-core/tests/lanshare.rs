//! Sharing a file on the network, from the other side: what a neighbour's
//! download manager meets at the link.

mod common;

use std::net::Ipv4Addr;
use std::time::Duration;

use dm_core::lanshare::{start_on, Share};
use dm_core::{client, download};

use common::{options, tempdir};

const SIZE: usize = 3 * 1024 * 1024 + 123;

fn data() -> Vec<u8> {
    (0..SIZE)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(5))
        .collect()
}

/// A share of a file called `name` with `data`, on the loopback.
fn shared(dir: &std::path::Path, name: &str, data: &[u8], lasts: Duration) -> Share {
    let file = dir.join(name);
    std::fs::write(&file, data).unwrap();
    start_on(&file, Ipv4Addr::LOCALHOST, Ipv4Addr::LOCALHOST, lasts).unwrap()
}

fn http() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().build().unwrap()
}

#[tokio::test]
async fn the_whole_file_comes_back_byte_for_byte() {
    let dir = tempdir();
    let want = data();
    let share = shared(dir.path(), "big one.bin", &want, Duration::from_secs(60));
    assert!(share.url().ends_with("/big%20one.bin"), "{}", share.url());
    let got = http().get(share.url()).send().await.unwrap();
    assert_eq!(got.status(), 200);
    assert_eq!(got.headers()["accept-ranges"], "bytes");
    assert!(got.headers()["content-disposition"]
        .to_str()
        .unwrap()
        .contains("big%20one.bin"));
    assert_eq!(got.bytes().await.unwrap().as_ref(), &want[..]);
    assert_eq!(share.sent(), SIZE as u64);
    assert_eq!(share.asked(), 1);
}

#[tokio::test]
async fn a_range_comes_back_as_that_part_alone() {
    let dir = tempdir();
    let want = data();
    let share = shared(dir.path(), "f.bin", &want, Duration::from_secs(60));
    for (header, from, to) in [
        ("bytes=10-99", 10, 100),
        ("bytes=-50", SIZE - 50, SIZE),
        ("bytes=3000000-", 3_000_000, SIZE),
    ] {
        let got = http()
            .get(share.url())
            .header("range", header)
            .send()
            .await
            .unwrap();
        assert_eq!(got.status(), 206, "{header}");
        assert_eq!(
            got.headers()["content-range"],
            format!("bytes {from}-{}/{SIZE}", to - 1).as_str()
        );
        assert_eq!(
            got.bytes().await.unwrap().as_ref(),
            &want[from..to],
            "{header}"
        );
    }
}

#[tokio::test]
async fn a_range_past_the_end_is_refused_in_words_the_client_knows() {
    let dir = tempdir();
    let share = shared(dir.path(), "f.bin", &data(), Duration::from_secs(60));
    let got = http()
        .get(share.url())
        .header("range", format!("bytes={SIZE}-"))
        .send()
        .await
        .unwrap();
    assert_eq!(got.status(), 416);
    assert_eq!(
        got.headers()["content-range"],
        format!("bytes */{SIZE}").as_str()
    );
}

#[tokio::test]
async fn a_head_says_the_size_and_sends_no_body() {
    let dir = tempdir();
    let share = shared(dir.path(), "f.bin", &data(), Duration::from_secs(60));
    let got = http().head(share.url()).send().await.unwrap();
    assert_eq!(got.status(), 200);
    assert_eq!(got.headers()["content-length"], SIZE.to_string().as_str());
    assert_eq!(share.sent(), 0);
}

#[tokio::test]
async fn nothing_else_is_reachable_through_the_link() {
    let dir = tempdir();
    std::fs::write(dir.path().join("secret.txt"), b"not shared").unwrap();
    let share = shared(dir.path(), "f.bin", &data(), Duration::from_secs(60));
    let base = share.url().rsplit_once('/').unwrap().0.to_string();
    let root = base.rsplit_once('/').unwrap().0.to_string();
    for wrong in [
        format!("{root}/"),
        format!("{root}/{}/f.bin", "0".repeat(32)),
        format!("{base}/../secret.txt"),
        format!("{root}/secret.txt"),
        format!("{root}/%2e%2e/secret.txt"),
    ] {
        let got = http().get(&wrong).send().await.unwrap();
        assert_eq!(got.status(), 404, "{wrong}");
        assert!(got.bytes().await.unwrap().is_empty());
    }
    let posted = http().post(share.url()).send().await.unwrap();
    assert_eq!(posted.status(), 405);
}

#[tokio::test]
async fn a_stopped_share_closes_its_port() {
    let dir = tempdir();
    let share = shared(dir.path(), "f.bin", &data(), Duration::from_secs(60));
    let url = share.url().to_string();
    assert_eq!(http().get(&url).send().await.unwrap().status(), 200);
    share.stop();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(http().get(&url).send().await.is_err());
}

#[tokio::test]
async fn a_share_ends_by_itself_when_its_time_is_up() {
    let dir = tempdir();
    let share = shared(dir.path(), "f.bin", &data(), Duration::from_millis(300));
    let url = share.url().to_string();
    assert_eq!(http().get(&url).send().await.unwrap().status(), 200);
    tokio::time::sleep(Duration::from_millis(800)).await;
    // The listener is gone with the accept loop's end: a new connection is
    // either refused or never answered.
    let late = http()
        .get(&url)
        .timeout(Duration::from_secs(2))
        .send()
        .await;
    assert!(late.is_err(), "{late:?}");
    drop(share);
}

#[tokio::test]
async fn only_a_file_can_be_shared() {
    let dir = tempdir();
    let folder = start_on(
        dir.path(),
        Ipv4Addr::LOCALHOST,
        Ipv4Addr::LOCALHOST,
        Duration::from_secs(5),
    );
    assert!(folder.is_err());
    let missing = start_on(
        &dir.path().join("gone.bin"),
        Ipv4Addr::LOCALHOST,
        Ipv4Addr::LOCALHOST,
        Duration::from_secs(5),
    );
    assert!(missing.is_err());
}

#[tokio::test]
async fn dm_fetches_a_share_over_several_connections_and_gets_the_same_file() {
    let dir = tempdir();
    let want = data();
    let share = shared(
        dir.path(),
        "movie night.bin",
        &want,
        Duration::from_secs(60),
    );
    let work = tempdir();
    let done = download(&client(), share.url(), &options(work.path()))
        .await
        .expect("dm downloads the share");
    assert_eq!(std::fs::read(&done.path).unwrap(), want);
    assert_eq!(
        std::path::Path::new(&done.path).file_name().unwrap(),
        "movie night.bin"
    );
    // One request to find out the size and resume, then the parts: more
    // than one connection came to the share.
    assert!(share.asked() > 1, "asked {} time(s)", share.asked());
}

/// By hand, from another computer: shares `DM_SHARE_FILE` on this one's real
/// network address, prints the link, and holds it open for `DM_SHARE_SECS`.
///
///     DM_SHARE_FILE=/some/file cargo test -p dm-core --test lanshare -- --ignored --nocapture lend
#[test]
#[ignore]
fn lend() {
    let file = std::env::var("DM_SHARE_FILE").expect("DM_SHARE_FILE names the file to share");
    let secs: u64 = std::env::var("DM_SHARE_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(120);
    let share = dm_core::lanshare::start(std::path::Path::new(&file), Duration::from_secs(secs))
        .expect("a share");
    println!("LINK {}", share.url());
    std::thread::sleep(Duration::from_secs(secs));
    println!("SENT {} bytes in {} requests", share.sent(), share.asked());
}

#[tokio::test]
async fn a_request_head_that_never_ends_is_dropped_not_buffered() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let dir = tempdir();
    let share = shared(dir.path(), "f.bin", &data(), Duration::from_secs(60));
    let host = share
        .url()
        .strip_prefix("http://")
        .unwrap()
        .split('/')
        .next()
        .unwrap();
    let mut stream = tokio::net::TcpStream::connect(host).await.unwrap();
    // Far more than a request head is ever, and no blank line to end it.
    let junk = vec![b'a'; 64 * 1024];
    let _ = stream.write_all(&junk).await;
    let mut got = Vec::new();
    let read = tokio::time::timeout(Duration::from_secs(3), stream.read_to_end(&mut got)).await;
    assert!(read.is_ok(), "the connection should have been closed");
    assert!(
        got.is_empty(),
        "nothing is said to a peer that sent no request"
    );
    // The share still serves.
    assert_eq!(http().get(share.url()).send().await.unwrap().status(), 200);
}

#[tokio::test]
async fn idle_connections_cannot_crowd_out_a_real_download() {
    let dir = tempdir();
    let want = data();
    let share = shared(dir.path(), "f.bin", &want, Duration::from_secs(60));
    let host = share
        .url()
        .strip_prefix("http://")
        .unwrap()
        .split('/')
        .next()
        .unwrap()
        .to_string();
    // A hundred peers that connect and say nothing.
    let mut idle = Vec::new();
    for _ in 0..100 {
        if let Ok(s) = tokio::net::TcpStream::connect(&host).await {
            idle.push(s);
        }
    }
    // The ones past the limit were closed at once; the share is full of the
    // others for ten seconds at most. Once they are let go, it answers.
    drop(idle);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let got = http().get(share.url()).send().await.unwrap();
    assert_eq!(got.status(), 200);
    assert_eq!(got.bytes().await.unwrap().as_ref(), &want[..]);
}

#[tokio::test]
async fn a_name_in_the_request_cannot_choose_a_file() {
    let dir = tempdir();
    std::fs::write(dir.path().join("other.txt"), b"another file").unwrap();
    let share = shared(dir.path(), "f.bin", &data(), Duration::from_secs(60));
    // Any name after the token gets the shared file, and only it.
    let base = share.url().rsplit_once('/').unwrap().0.to_string();
    for name in [
        "other.txt",
        "..%2fother.txt",
        "%2e%2e/other.txt",
        "x/../other.txt",
    ] {
        let got = http().get(format!("{base}/{name}")).send().await.unwrap();
        if got.status() == 200 {
            assert_eq!(got.bytes().await.unwrap().len(), SIZE, "{name}");
        }
    }
}
