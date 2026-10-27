//! What a failed yt-dlp run means, in words a person can act on.

/// What a run ends with when yt-dlp has no extractor for the address: the
/// one refusal after which the address may still be an ordinary file.
pub const UNKNOWN_SITE: &str = "yt-dlp does not know this site";

/// What a site's refusal means, in words a person can act on, when yt-dlp's
/// error output shows one of the common ones. A live run against YouTube
/// failed with yt-dlp's raw "HTTP Error 429" after Google sent it to its
/// "sorry" page; the row should say the network was rate-limited, and that
/// waiting is the fix, not that dm broke.
pub fn refusal(stderr: &str) -> Option<String> {
    let said = stderr.to_ascii_lowercase();
    let has = |needle: &str| said.contains(needle);
    if has("http error 429") || has("too many requests") || has("google.com/sorry") {
        return Some(
            "the site is limiting downloads from this network right now (HTTP 429); \
             try again later"
                .into(),
        );
    }
    if has("confirm you") && has("not a bot") {
        return Some(
            "the site wants this network to prove it is not a bot; open the video in \
             the browser, or try again later"
                .into(),
        );
    }
    if has("sign in") || has("log in") || has("login required") || has("logged-in") {
        return Some("the site only shows this to someone signed in".into());
    }
    if has("private video") {
        return Some("that video is private".into());
    }
    if has("video unavailable")
        || has("this video is unavailable")
        || has("this video has been removed")
        || has("http error 404")
    {
        return Some("that video is not available any more".into());
    }
    if has("unsupported url") {
        return Some(UNKNOWN_SITE.into());
    }
    // A stream encrypted for the site's own player alone. Neither yt-dlp
    // nor dm takes protection off, and yt-dlp's advice to pick another
    // format leads nowhere when every format is protected (Vimeo's).
    if has("drm protected") || has("drm-protected") {
        return Some(
            "that video is copy-protected (DRM), so only the site's own player can play it; \
             dm does not download protected video"
                .into(),
        );
    }
    // YouTube's player script has to be run to read some or all of a
    // video's formats; yt-dlp says so in these words. Last, since a sign-in or a private video is the
    // better thing to say where it is the cause; cookies do not help here.
    if has("solving failed")
        || has("challenge solver")
        || has("no supported javascript runtime")
        || has("only images are available")
    {
        return Some(
            "yt-dlp could not run the site's player script, so it found no formats it can \
             fetch (a sign-in does not fix this): update yt-dlp, or install Deno \
             (deno.com) for it to run the script with"
                .into(),
        );
    }
    None
}

/// What to say of a failed yt-dlp run: its refusal in plain words where it
/// is a known one, else its own last error line, else `fallback`. Never the
/// whole output: it can carry addresses.
pub fn said_by(stderr: &str, fallback: &str) -> String {
    if let Some(said) = refusal(stderr) {
        return said;
    }
    let line = stderr
        .lines()
        .rev()
        .find_map(|l| l.trim().strip_prefix("ERROR:"))
        .map(|l| l.trim().chars().take(200).collect::<String>())
        .filter(|l| !l.is_empty());
    match line {
        Some(line) => format!("{fallback}: {line}"),
        None => fallback.to_string(),
    }
}
