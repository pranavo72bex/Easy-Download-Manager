//! The tools dm hands work to, found where people install them. Run by CI
//! after it installs yt-dlp as a person would (on Windows, `pip install
//! --user`, whose Scripts folder is not on the PATH a dm started by a
//! browser or the Start menu has); skipped everywhere else.

/// Issue 10: an installed yt-dlp was reported missing on Windows.
#[test]
fn an_installed_yt_dlp_is_found() {
    if std::env::var_os("DM_EXPECT_YTDLP").is_none() {
        eprintln!("DM_EXPECT_YTDLP not set — skipping");
        return;
    }
    assert!(
        dm_core::ytdlp::available(),
        "yt-dlp is installed (DM_EXPECT_YTDLP) and dm did not find it; PATH is {:?}",
        std::env::var_os("PATH")
    );
}
