//! Every yt-dlp error line in `tests/fixtures/ytdlp-errors.txt` is said in
//! dm's own words (docs/gaps.md row 36): a known refusal as its sentence, an
//! unknown one as the fallback with the site named and yt-dlp's words in
//! brackets, and never yt-dlp's advice, which is for people running yt-dlp
//! themselves: its flags, its issue tracker, its update command.

use std::path::Path;

#[test]
fn every_line_of_the_corpus_is_said_in_dms_words_without_yt_dlps_advice() {
    let corpus = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ytdlp-errors.txt"),
    )
    .unwrap();
    let mut seen = 0;
    for line in corpus.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let (said_by_ytdlp, expected) = line
            .split_once('\t')
            .unwrap_or_else(|| panic!("no tab in {line:?}"));
        let said =
            dm_core::ytdlp::said_by(said_by_ytdlp, "the video helper could not download that");
        assert!(
            said.contains(expected),
            "for {said_by_ytdlp:?}\n   dm said {said:?}\n   wanted  {expected:?}"
        );
        for advice in [
            "--",
            "github.com",
            "please report",
            "yt-dlp -U",
            "http://",
            "https://",
        ] {
            assert!(
                !said.contains(advice),
                "yt-dlp's advice reached the person: {advice:?} in {said:?}"
            );
        }
        seen += 1;
    }
    assert!(seen >= 30, "the corpus is {seen} lines; it was 38");
}

/// A refusal reaches the download's message as its sentence, and the
/// fallback keeps the shape the window reads (`format.rs`): the download's
/// own line, not the bracketed one.
#[test]
fn a_known_refusal_is_the_same_sentence_for_a_read_and_a_download() {
    let stderr = "ERROR: [youtube] abc: This video requires payment to watch.";
    assert_eq!(
        dm_core::ytdlp::refusal(stderr).as_deref(),
        Some("that video has to be bought or rented on the site")
    );
    assert_eq!(
        dm_core::ytdlp::said_by(stderr, "anything"),
        "that video has to be bought or rented on the site"
    );
}
