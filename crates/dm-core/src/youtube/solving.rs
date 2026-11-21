// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The player's checks answered: each n value and signature the formats carry, by the player script
//! cut down and run in the computer's own JavaScript engine.

use super::*;

/// The few entities a page's title carries, read back.
pub(super) fn unescaped(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

/// Whether an address carries an `n` value to solve.
pub(super) fn has_n(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|u| u.query_pairs().any(|(k, _)| k == "n"))
}

/// The address a stream is fetched from: its cipher decoded and its `n`
/// value replaced, from what the solver answered.
pub(super) fn unlocked(format: &Format, solved: &Solved) -> Result<String, Error> {
    let (mut url, signature) = match (&format.url, &format.cipher) {
        (Some(url), _) => (parsed(url)?, None),
        (None, Some(cipher)) => {
            let pairs: HashMap<String, String> = url::form_urlencoded::parse(cipher.as_bytes())
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect();
            let url = pairs
                .get("url")
                .ok_or_else(|| Error::Internal("a cipher with no address".into()))?;
            let s = pairs
                .get("s")
                .ok_or_else(|| Error::Internal("a cipher with no signature".into()))?;
            let sig = solved
                .sig
                .get(s)
                .ok_or_else(|| Error::Internal("the signature was not solved".into()))?;
            let name = pairs
                .get("sp")
                .cloned()
                .unwrap_or_else(|| "signature".into());
            (parsed(url)?, Some((name, sig.clone())))
        }
        (None, None) => return Err(Error::Internal("a stream with no address".into())),
    };
    let mut pairs: Vec<(String, String)> = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    for (key, value) in &mut pairs {
        if key == "n" {
            if let Some(answer) = solved.n.get(value) {
                *value = answer.clone();
            }
        }
    }
    if let Some((name, sig)) = signature {
        pairs.push((name, sig));
    }
    url.query_pairs_mut().clear().extend_pairs(pairs);
    Ok(url.to_string())
}

/// `address` with its `n` value replaced by the solver's answer, where it carries one: the
/// framed stream's address (gaps row Y2).
pub(super) fn n_solved(address: &str, solved: &Solved) -> Result<String, Error> {
    let mut url = parsed(address)?;
    let pairs: Vec<(String, String)> = url
        .query_pairs()
        .map(|(k, v)| {
            let answer = if k == "n" {
                solved.n.get(&*v).cloned()
            } else {
                None
            };
            (k.into_owned(), answer.unwrap_or_else(|| v.into_owned()))
        })
        .collect();
    url.query_pairs_mut().clear().extend_pairs(pairs);
    Ok(url.to_string())
}

/// An address parsed, or why not in dm's words.
fn parsed(url: &str) -> Result<url::Url, Error> {
    url::Url::parse(url)
        .map_err(|e| Error::Internal(format!("a stream's address is not one ({e})")))
}

/// The build of the player script every engine reads, where the page named another: the one
/// in the JavaScript older engines know (`player_ias`) for the newer one (`player_es6`, whose
/// syntax Windows' own engine predates). The same version, so the same answers.
pub(super) fn plain_build(path: &str) -> Option<String> {
    path.contains("/player_es6.vflset/")
        .then(|| path.replace("/player_es6.vflset/", "/player_ias.vflset/"))
}

/// Answers every `n` value and signature the formats carry, and the `n` value of the `also`
/// address (the framed stream's), as reference A answers them: the player script cut down to
/// what answers them (`player::program`), run in the computer's own JavaScript engine
/// (`crate::javascript`).
pub(super) async fn solve(
    player_js: &str,
    formats: &[Format],
    also: Option<&str>,
) -> Result<Solved, Error> {
    let mut ns = BTreeSet::new();
    let mut sigs = BTreeSet::new();
    let mut n_of = |address: &str| {
        if let Ok(url) = url::Url::parse(address) {
            ns.extend(
                url.query_pairs()
                    .filter(|(k, _)| k == "n")
                    .map(|(_, v)| v.into_owned()),
            );
        }
    };
    if let Some(url) = also {
        n_of(url);
    }
    for format in formats {
        if let Some(url) = format.url.as_deref() {
            n_of(url);
        }
        // A locked address carries its `n` inside the cipher.
        if let Some(cipher) = &format.cipher {
            for (key, value) in url::form_urlencoded::parse(cipher.as_bytes()) {
                match &*key {
                    "s" => {
                        sigs.insert(value.into_owned());
                    }
                    "url" => n_of(&value),
                    _ => {}
                }
            }
        }
    }
    let ns: Vec<&str> = ns.iter().map(String::as_str).collect();
    let sigs: Vec<&str> = sigs.iter().map(String::as_str).collect();
    answer(player_js, &ns, &sigs).await
}

/// What the player script `player_js` answers for `ns` (`n` values) and `sigs` (signatures),
/// each by the value asked, cut and run as a download's reading runs it: for a person looking
/// into a player that changed (`examples/player_answers.rs`).
#[doc(hidden)]
pub async fn answers(
    player_js: &str,
    ns: &[&str],
    sigs: &[&str],
) -> Result<(HashMap<String, String>, HashMap<String, String>), Error> {
    let solved = answer(player_js, ns, sigs).await?;
    Ok((solved.n, solved.sig))
}

/// [`answers`], as the reading takes them.
async fn answer(player_js: &str, ns: &[&str], sigs: &[&str]) -> Result<Solved, Error> {
    let unanswered =
        |why: String| Error::Internal(format!("YouTube's checks were not answered: {why}"));
    let program = cut_program(player_js).map_err(unanswered)?;
    let call = player::call(ns, sigs);
    let output = crate::javascript::run(&program, &call, SOLVING)
        .await
        .map_err(unanswered)?;
    let answer: Value = serde_json::from_str(output.trim())
        .map_err(|_| unanswered(format!("the answer was {:?}", output.trim())))?;
    let mut solved = Solved::default();
    for (kind, asked, into) in [("n", ns, &mut solved.n), ("sig", sigs, &mut solved.sig)] {
        for challenge in asked.iter() {
            let got = answer
                .pointer(&format!(
                    "/{kind}/{}",
                    challenge.replace('~', "~0").replace('/', "~1")
                ))
                .and_then(Value::as_str)
                .filter(|got| !got.is_empty())
                .ok_or_else(|| unanswered(format!("no answer for the {kind} value {challenge}")))?;
            into.insert((*challenge).to_string(), got.to_string());
        }
    }
    Ok(solved)
}

/// The reading as JSON: what `videopage::download_page` reads of one
/// (`filename`, `requested_formats` or the one format, each with its
/// address, type, agent, chunk and size) and what the panel's list reads
/// (`formats` with heights, codecs and sizes).
pub(super) fn reading_json(
    id: &str,
    title: &str,
    duration: Option<u64>,
    formats: &[Format],
    chosen: &[Format],
    staging: &Path,
) -> Value {
    let describe = |f: &Format| {
        let mut described = json!({
            "format_id": f.itag.to_string(),
            "url": f.url,
            // Numbered pieces (`&sq=N`), fetched by the DASH piece fetcher; else one file.
            // Or neither address nor cipher: the framed stream's alone (gaps row Y2).
            "protocol": if f.framed {
                framed::PROTOCOL
            } else if f.numbered {
                "http_dash_segments"
            } else {
                "https"
            },
            "ext": f.ext(),
            "vcodec": if f.video { f.codec.as_str() } else { "none" },
            "acodec": if f.video { "none" } else { f.codec.as_str() },
            "width": if f.video { Some(f.width) } else { None },
            "height": if f.video { Some(f.height) } else { None },
            "fps": if f.video && f.fps > 0 { Some(f.fps) } else { None },
            "tbr": f.bitrate / 1000,
            // A sound's own rate, by which the list built from this reading picks its best.
            "abr": if f.video { None } else { Some(f.bitrate / 1000) },
            "filesize": f.size,
            "http_headers": {"User-Agent": TV_AGENT},
            "downloader_options": {"http_chunk_size": CHUNK},
            // dm's own, for cutting a part: where the stream's first boxes
            // and its index are.
            "dm_init_range": f.init.map(|(a, b)| format!("{a}-{b}")),
            "dm_index_range": f.index.map(|(a, b)| format!("{a}-{b}")),
        });
        // Numbered pieces state no size: about, from the bitrate and the video's length, for
        // the quality list's sizes.
        if let (true, None, Some(seconds)) = (f.numbered, f.size, duration) {
            described["filesize_approx"] = json!(f.bitrate / 8 * seconds);
        }
        described
    };
    let ext = chosen
        .first()
        .map(|f| f.ext().to_string())
        .unwrap_or_else(|| "mp4".into());
    let name = format!("{} [{id}].{ext}", safe_title(title));
    let mut reading = json!({
        "id": id,
        "title": title,
        "extractor": "youtube",
        "extractor_key": "Youtube",
        "webpage_url": format!("https://www.youtube.com/watch?v={id}"),
        "duration": duration,
        "ext": ext,
        "filename": staging.join(name).display().to_string(),
        "formats": formats.iter().map(describe).collect::<Vec<_>>(),
        "read_by": "dm",
    });
    match chosen {
        [one] => {
            if let (Value::Object(into), Value::Object(from)) = (&mut reading, describe(one)) {
                for (key, value) in from {
                    into.insert(key, value);
                }
            }
        }
        pair => {
            reading["requested_formats"] = pair.iter().map(describe).collect();
        }
    }
    reading
}
