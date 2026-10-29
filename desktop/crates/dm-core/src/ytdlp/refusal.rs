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
    if has("private video") || has("this video is private") {
        return Some("that video is private".into());
    }
    if has("confirm your age") {
        return Some(
            "that video is age-restricted: the site shows it only to someone signed in".into(),
        );
    }
    if has("sign in")
        || has("log in")
        || has("login required")
        || has("logged-in")
        || has("requires authentication")
        || has("authentication required")
    {
        return Some("the site only shows this to someone signed in".into());
    }
    if has("video unavailable")
        || has("this video is unavailable")
        || has("this video has been removed")
        || has("no longer available")
        || has("video not available")
        || has("http error 404")
    {
        return Some("that video is not available any more".into());
    }
    if has("available in your country") {
        return Some("that video is not available in this country".into());
    }
    if has("live event will begin") || has("premieres in") || has("premiere will begin") {
        return Some("that live stream has not started yet; try again once it has".into());
    }
    if has("live stream recording is not available") {
        return Some("that live stream's recording is not available".into());
    }
    if has("members-only") || has("channel's members") || has("join this channel") {
        return Some("that video is for the channel's members only".into());
    }
    if has("requires payment") {
        return Some("that video has to be bought or rented on the site".into());
    }
    if has("protected by a password") || has("--video-password") {
        return Some("that video needs a password, which dm cannot give the site".into());
    }
    if has("embed-only") || has("embedding url") {
        return Some(
            "that video plays only on the page it is embedded in; add that page's address instead"
                .into(),
        );
    }
    if has("looks truncated") || has("incomplete youtube id") {
        return Some("that address is not a whole video link".into());
    }
    if has("forcing sabr") {
        return Some(
            "the site now streams this in a way yt-dlp cannot fetch yet; dm fetches a newer \
             yt-dlp when there is one"
                .into(),
        );
    }
    if has("unable to download webpage")
        && (has("urlerror")
            || has("errno")
            || has("nodename")
            || has("name or service")
            || has("connection")
            || has("timed out"))
    {
        return Some("the site could not be reached; check the connection and try again".into());
    }
    if has("unable to download video data") && has("http error 403") {
        return Some(
            "the site refused the stream (403): its address may have run out; try again, or \
             open the video in the browser first"
                .into(),
        );
    }
    if has("no space left on device") {
        return Some("the disk is full".into());
    }
    if has("ffmpeg not found") {
        return Some(
            "this one needs ffmpeg to be put together, which is not installed (ffmpeg.org)".into(),
        );
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
    if has("requested format is not available") {
        return Some(
            "the site listed no format yt-dlp could fetch, which is usually a change on the \
             site's side; dm fetches a newer yt-dlp when there is one"
                .into(),
        );
    }
    None
}

/// The last `ERROR:` line of a run, read: the site yt-dlp's reader names
/// in brackets (`[youtube]`, `[youtube:tab]`), shown as people write it,
/// and the message without the video's id before it, yt-dlp's request to
/// report the issue to it, the flags it suggests, and its links: none of
/// them is for a person using dm.
fn last_error(stderr: &str) -> Option<(Option<String>, String)> {
    let line = stderr
        .lines()
        .rev()
        .find_map(|l| l.trim().strip_prefix("ERROR:"))
        .map(str::trim)
        .filter(|l| !l.is_empty())?;
    let (site, rest) = match line.strip_prefix('[') {
        Some(tagged) => {
            let (tag, rest) = tagged.split_once(']')?;
            let tag = tag.split(':').next().unwrap_or(tag).trim();
            (site_name(tag), rest.trim())
        }
        None => (None, line),
    };
    // "<id>: <message>": the id is one word, and a message never is.
    let message = match rest.split_once(": ") {
        Some((id, message)) if !id.contains(' ') && !id.is_empty() => message,
        _ => rest,
    };
    let mut message = message.trim();
    for stop in [
        "; please report",
        ". please report",
        " please report",
        ". Use --",
        " Use --",
        ", use the --",
        ". See ",
        " See ",
        " (caused by",
        ". Please call",
        ". Please install",
    ] {
        if let Some(i) = message.find(stop) {
            message = &message[..i];
        }
    }
    let message = message.trim().trim_end_matches(['.', ',', ';']).trim();
    if message.is_empty() {
        return None;
    }
    Some((site, message.chars().take(160).collect()))
}

/// A site's name as people write it, from yt-dlp's tag for its reader.
/// `generic` names no site. Another is the tag with its first letter up.
fn site_name(tag: &str) -> Option<String> {
    let known = [
        ("youtube", "YouTube"),
        ("vimeo", "Vimeo"),
        ("twitter", "X"),
        ("instagram", "Instagram"),
        ("tiktok", "TikTok"),
        ("facebook", "Facebook"),
        ("dailymotion", "Dailymotion"),
        ("twitch", "Twitch"),
        ("reddit", "Reddit"),
        ("soundcloud", "SoundCloud"),
        ("bilibili", "Bilibili"),
        ("rumble", "Rumble"),
        ("odysee", "Odysee"),
    ];
    let lower = tag.to_ascii_lowercase();
    if lower == "generic" || lower.is_empty() {
        return None;
    }
    if let Some((_, name)) = known.iter().find(|(t, _)| *t == lower) {
        return Some((*name).to_string());
    }
    let mut chars = tag.chars();
    Some(chars.next()?.to_uppercase().chain(chars).collect())
}

/// What to say of a failed yt-dlp run: its refusal in plain words where it
/// is a known one; else `fallback`, the site named where yt-dlp named it,
/// and yt-dlp's own words in brackets, without its advice (`last_error`);
/// else `fallback` alone. Never the whole output: it can carry addresses.
pub fn said_by(stderr: &str, fallback: &str) -> String {
    if let Some(said) = refusal(stderr) {
        return said;
    }
    match last_error(stderr) {
        Some((Some(site), message)) => format!("{fallback} on {site} (yt-dlp: {message})"),
        Some((None, message)) => format!("{fallback} (yt-dlp: {message})"),
        None => fallback.to_string(),
    }
}
