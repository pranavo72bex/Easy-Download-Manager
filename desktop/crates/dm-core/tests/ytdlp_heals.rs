//! A yt-dlp that fails in words that are not a site's refusal is replaced by
//! the newest release, where there is one, and the run made once more
//! (docs/gaps.md row 33): a site changed under it, and the fix is out. Its
//! own test binary: it says through the environment where yt-dlp's releases
//! are (a server of its own) and that only fetched tools count.
#![cfg(unix)]

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::routing::get;
use sha2::Digest as _;

use common::{options, serve, tempdir};

/// yt-dlp's unpacked build for this computer, as a zip: the program named
/// as the archive is, and a library beside it.
fn build(program: &str, asset: &str) -> Vec<u8> {
    use std::io::Write as _;
    let mut out = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut out);
    let file = |mode| zip::write::SimpleFileOptions::default().unix_permissions(mode);
    zip.start_file(asset.trim_end_matches(".zip"), file(0o755))
        .unwrap();
    zip.write_all(program.as_bytes()).unwrap();
    zip.start_file("_internal/lib.so", file(0o644)).unwrap();
    zip.write_all(b"library").unwrap();
    zip.finish().unwrap();
    out.into_inner()
}

/// A release that fails every run in yt-dlp's words for a site it can no
/// longer read, and the next release, which downloads.
const BROKEN: &str = "#!/bin/sh\necho 'ERROR: [youtube] abc: Unable to extract yt initial data; please report this issue' >&2\nexit 1\n";
const FIXED: &str = "#!/bin/sh\nout=''; res=''; prev=''; prev2=''\nfor a in \"$@\"; do\n  if [ \"$prev\" = '-o' ]; then out=$(dirname \"$a\"); fi\n  if [ \"$prev2\" = '--print-to-file' ]; then res=\"$a\"; fi\n  prev2=\"$prev\"; prev=\"$a\"\ndone\nmkdir -p \"$out\"; printf 'fixed' > \"$out/fixed [x].mp4\"; printf '%s\\n' \"$out/fixed [x].mp4\" > \"$res\"\n";

#[tokio::test(flavor = "multi_thread")]
async fn a_yt_dlp_that_fails_on_a_changed_site_is_replaced_by_the_newest_and_tried_once_more() {
    let asset = dm_core::tools::ytdlp_asset(std::env::consts::OS, std::env::consts::ARCH)
        .expect("a build for this computer");
    let releases: Arc<Mutex<(Vec<u8>, String)>> = Arc::new(Mutex::new((Vec::new(), String::new())));
    let publish = |releases: &Arc<Mutex<(Vec<u8>, String)>>, program: &str| {
        let zip = build(program, asset);
        let digest = hex::encode(sha2::Sha256::digest(&zip));
        *releases.lock().unwrap() = (zip, format!("{digest}  {asset}\n"));
    };
    publish(&releases, BROKEN);
    let fetched = Arc::new(AtomicUsize::new(0));
    let (r1, r2, f) = (releases.clone(), releases.clone(), fetched.clone());
    let app = axum::Router::new()
        .route(
            "/y/SHA2-256SUMS",
            get(move || {
                let r = r1.clone();
                async move { r.lock().unwrap().1.clone() }
            }),
        )
        .route(
            &format!("/y/{asset}"),
            get(move || {
                let (r, f) = (r2.clone(), f.clone());
                async move {
                    f.fetch_add(1, Ordering::SeqCst);
                    r.lock().unwrap().0.clone()
                }
            }),
        );
    let base = format!("http://{}", serve(app).await);

    let dir = tempdir();
    let tools = dir.path().join("tools");
    std::fs::create_dir_all(&tools).unwrap();
    dm_core::tools::set_folder(tools.clone());
    std::env::set_var("DM_ONLY_FETCHED_TOOLS", "1");
    std::env::set_var("DM_YTDLP_RELEASES", format!("{base}/y"));
    let client = dm_core::client();

    // The first video: the release of the day is fetched, and it is the broken one.
    dm_core::tools::ensure(&client, dm_core::tools::Tool::YtDlp)
        .await
        .expect("the first fetch");
    assert_eq!(fetched.load(Ordering::SeqCst), 1);
    // yt-dlp's fix comes out.
    publish(&releases, FIXED);

    let done = dm_core::ytdlp::download_ytdlp(
        "https://www.youtube.com/watch?v=abc",
        &options(dir.path()),
        None,
    )
    .await
    .expect("the download, after the newer yt-dlp was fetched");
    assert_eq!(std::fs::read(&done.path).unwrap(), b"fixed");
    assert_eq!(
        fetched.load(Ordering::SeqCst),
        2,
        "the newer release was fetched once"
    );

    // Another failure within the hour fetches nothing more, whatever is published.
    publish(&releases, BROKEN);
    assert!(
        !dm_core::tools::newer_ytdlp(&client).await,
        "a second try within the hour"
    );
    assert_eq!(fetched.load(Ordering::SeqCst), 2);
}
