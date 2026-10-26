//! A transport-stream picture with its sound in packed AAC.
//!
//! An HLS master may keep its sound in a playlist of its own whose segments
//! are bare ADTS frames (`.aac`), each segment led by an ID3 tag that says,
//! in the 90 kHz clock the picture's stamps use, when the segment starts.
//! The frames are read exactly as a transport stream's AAC is, the first
//! tag is the sound's start, and both go to the MP4 writer as the two
//! tracks of one file. Nothing is decoded or encoded.

use super::*;

/// What the ID3 tag of a segment names its clock by.
const OWNER: &[u8] = b"com.apple.streaming.transportStreamTimestamp\0";

/// Puts the transport stream at `ts` and the packed AAC at `aac` into one
/// MP4 at `out`, with scratch files beside `out` that are gone when it
/// returns. Sound inside the transport stream, if it has any, is dropped
/// for the packed one. Returns its length.
pub fn join_packed(ts: &Path, aac: &Path, out: &Path) -> Result<u64, String> {
    let scratch = |what: &str| {
        let mut name = out.as_os_str().to_owned();
        name.push(format!(".{what}"));
        PathBuf::from(name)
    };
    let mut streams = read_ts(ts, &scratch)?;
    let result = (|| {
        streams.retain(|s| {
            let keep = PICTURES.contains(&s.kind);
            if !keep {
                let _ = std::fs::remove_file(&s.path);
            }
            keep
        });
        if !streams.iter().any(|s| !s.samples.is_empty()) {
            return Err("its transport stream holds no picture dm can put into an MP4".into());
        }
        streams.push(read_packed(aac, scratch("packed"))?);
        build(&mut streams, out)
    })();
    for stream in &streams {
        let _ = std::fs::remove_file(&stream.path);
    }
    result
}

/// [`join_packed`], off the async runtime.
pub async fn join_packed_async(ts: PathBuf, aac: PathBuf, out: PathBuf) -> Result<u64, String> {
    tokio::task::spawn_blocking(move || join_packed(&ts, &aac, &out))
        .await
        .map_err(|e| format!("the remux stopped: {e}"))?
}

/// Whether the file at `path` starts as packed AAC does: an ID3 tag or an
/// ADTS frame.
pub fn is_packed_aac(path: &Path) -> bool {
    let mut head = [0u8; 10];
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    if file.read_exact(&mut head).is_err() {
        return false;
    }
    id3_length(&head).is_some() || matches!(adts_frame(&head), Frame::Whole { .. } | Frame::Short)
}

/// Whether the file at `path` starts as a transport stream does.
pub fn is_transport_stream(path: &Path) -> bool {
    let mut head = [0u8; 1];
    File::open(path)
        .and_then(|mut f| f.read_exact(&mut head))
        .is_ok()
        && head == [0x47]
}

fn read_packed(input: &Path, scratch: PathBuf) -> Result<Elementary, String> {
    let mut file = File::open(input).map_err(|e| format!("the sound could not be read: {e}"))?;
    let out =
        File::create(&scratch).map_err(|e| format!("a scratch file could not be made: {e}"))?;
    let mut stream = Elementary {
        kind: AAC,
        out: Some(BufWriter::with_capacity(1 << 20, out)),
        path: scratch,
        ..Elementary::default()
    };
    let mut pending: Vec<u8> = Vec::new();
    let mut chunk = vec![0u8; 1 << 20];
    let mut stamp: Option<i64> = None;
    let mut ended = false;
    while !ended {
        let n = file
            .read(&mut chunk)
            .map_err(|e| format!("the sound could not be read: {e}"))?;
        ended = n == 0;
        pending.extend_from_slice(chunk.get(..n).unwrap_or_default());
        let mut at = 0;
        loop {
            let rest = pending.get(at..).unwrap_or_default();
            if let Some(tag) = id3_length(rest) {
                // The tag's end has not arrived: wait for it.
                let Some(body) = rest.get(10..tag) else {
                    if ended {
                        at = pending.len();
                    }
                    break;
                };
                if stamp.is_none() {
                    stamp = timestamp_in(body);
                }
                at += tag;
                continue;
            }
            // Frames up to the next tag, or all but what could be the start
            // of one while more may come.
            let next = (1..rest.len()).find(|i| rest.get(*i..).and_then(id3_length).is_some());
            let upto = match next {
                Some(i) => i,
                None if ended => rest.len(),
                None => rest.len().saturating_sub(9),
            };
            if upto == 0 {
                break;
            }
            take_sound(&mut stream, rest.get(..upto).unwrap_or_default(), stamp)?;
            at += upto;
            if next.is_none() {
                break;
            }
        }
        pending.drain(..at.min(pending.len()));
    }
    if let Some(mut writer) = stream.out.take() {
        writer
            .flush()
            .map_err(|e| format!("a scratch file could not be written: {e}"))?;
    }
    if stream.samples.is_empty() {
        return Err("its sound holds no AAC frames".into());
    }
    Ok(stream)
}

/// The length of the ID3 tag (header included) that `data` starts with.
fn id3_length(data: &[u8]) -> Option<usize> {
    let head = data.get(..10)?;
    if head.get(..3)? != b"ID3" || head.get(3).copied()? == 0xff {
        return None;
    }
    let size = head.get(6..10)?;
    if size.iter().any(|b| b & 0x80 != 0) {
        return None;
    }
    let body = size.iter().fold(0usize, |n, b| (n << 7) | usize::from(*b));
    Some(10 + body)
}

/// The start the tag's private frame states, in the 90 kHz clock.
fn timestamp_in(body: &[u8]) -> Option<i64> {
    let at = body.windows(OWNER.len()).position(|w| w == OWNER)? + OWNER.len();
    let raw: [u8; 8] = body.get(at..at + 8)?.try_into().ok()?;
    Some((u64::from_be_bytes(raw) & (WRAP as u64 - 1)) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(stamp: u64) -> Vec<u8> {
        let mut frame = OWNER.to_vec();
        frame.extend_from_slice(&stamp.to_be_bytes());
        let mut body = b"PRIV".to_vec();
        body.extend_from_slice(&(frame.len() as u32).to_be_bytes());
        body.extend_from_slice(&[0, 0]);
        body.extend(frame);
        let mut out = b"ID3\x04\x00\x00".to_vec();
        let n = body.len();
        out.extend_from_slice(&[
            (n >> 21) as u8 & 0x7f,
            (n >> 14) as u8 & 0x7f,
            (n >> 7) as u8 & 0x7f,
            n as u8 & 0x7f,
        ]);
        out.extend(body);
        out
    }

    #[test]
    fn a_tag_gives_its_length_and_the_start_it_states() {
        let t = tag(123_456);
        assert_eq!(id3_length(&t), Some(t.len()));
        assert_eq!(timestamp_in(&t[10..]), Some(123_456));
        assert_eq!(id3_length(b"ID3\x04\x00\x00\x80\0\0\0"), None);
        assert_eq!(id3_length(b"not a tag!"), None);
    }

    #[test]
    fn frames_are_read_across_tags_and_chunks() {
        // 44.1 kHz stereo AAC-LC: 0xFFF1, profile 1, rate 4, channels 2.
        let frame = |len: usize| {
            let mut f = vec![
                0xff,
                0xf1,
                0x50,
                0x80 | ((len >> 11) & 3) as u8,
                (len >> 3) as u8,
                ((len & 7) << 5) as u8 | 0x1f,
                0xfc,
            ];
            f.resize(len, 7);
            f
        };
        let dir = tempfile::tempdir().unwrap();
        let mut data = Vec::new();
        for segment in 0..3u64 {
            data.extend(tag(90_000 * (segment + 1)));
            for _ in 0..40 {
                data.extend(frame(300));
            }
        }
        let input = dir.path().join("sound.aac");
        std::fs::write(&input, &data).unwrap();
        assert!(is_packed_aac(&input));
        let stream = read_packed(&input, dir.path().join("scratch")).unwrap();
        assert_eq!(stream.samples.len(), 120);
        assert_eq!(stream.first_pts, Some(90_000));
        assert!(stream.samples.iter().all(|s| s.1 == 293));
    }
}
