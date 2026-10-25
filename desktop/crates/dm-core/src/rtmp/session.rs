//! Recording a stream from the first command to its end.

use super::*;

/// How a session ended.
pub(super) enum Ended {
    /// The server said the stream is over, or closed the connection.
    Over,
    Stopped(Error),
}

/// Records the stream at `url` into `part` as a fragmented MP4 until its
/// server ends it, the connection closes, or `control` stops it. Returns
/// the bytes written; a stop is returned after the file is closed, so what
/// arrived plays.
pub async fn record(
    url: &str,
    part: &Path,
    control: &Control,
    downloaded: &Arc<AtomicU64>,
) -> Result<u64, Error> {
    let target = target(url).map_err(Error::Internal)?;
    let mut io = open(&target).await?;
    handshake(&mut *io).await?;
    let (read_half, mut write_half) = tokio::io::split(io);
    let mut reader = Reader {
        io: BufReader::with_capacity(64 * 1024, read_half),
        chunk_size: 128,
        streams: HashMap::new(),
        read: 0,
    };
    let out_chunk = 4096usize;
    let mut send = Vec::new();
    send.extend(chunks(2, 1, 0, 0, &(out_chunk as u32).to_be_bytes(), 128));
    let connect = command(
        "connect",
        1.0,
        &[&|out: &mut Vec<u8>| {
            amf_object(
                out,
                &[
                    ("app", Amf::String(target.app.clone())),
                    ("flashVer", Amf::String("LNX 9,0,124,2".into())),
                    ("tcUrl", Amf::String(target.tc_url.clone())),
                    ("fpad", Amf::Bool(false)),
                    ("capabilities", Amf::Number(15.0)),
                    ("audioCodecs", Amf::Number(4071.0)),
                    ("videoCodecs", Amf::Number(252.0)),
                    ("videoFunction", Amf::Number(1.0)),
                ],
            )
        }],
    );
    send.extend(chunks(3, 20, 0, 0, &connect, out_chunk));
    write_half.write_all(&send).await?;

    let mut recorder = Recorder::new(part);
    let mut window: Option<u64> = None;
    let mut acked = 0u64;
    let mut playing = false;
    let ended = loop {
        let next = tokio::select! {
            m = tokio::time::timeout(Duration::from_secs(30), reader.message()) => m,
            _ = tokio::time::sleep(Duration::from_millis(250)) => {
                if let Err(stop) = control.check() {
                    break Ended::Stopped(stop);
                }
                continue;
            }
        };
        let message = match next {
            Ok(Ok(m)) => m,
            // Closed: a server that ends a stream may just hang up.
            Ok(Err(Error::Disk(e))) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break Ended::Over;
            }
            Ok(Err(e)) => break Ended::Stopped(e),
            Err(_) if recorder.written() > 0 => break Ended::Over,
            Err(_) => {
                break Ended::Stopped(Error::Internal(
                    "the server sent nothing for 30 seconds".into(),
                ))
            }
        };
        if let Some(window) = window {
            if reader.read - acked >= window / 2 {
                acked = reader.read;
                let ack = chunks(2, 3, 0, 0, &(acked as u32).to_be_bytes(), out_chunk);
                write_half.write_all(&ack).await?;
            }
        }
        let body = &message.body;
        match message.kind {
            1 => {
                let size = body.get(..4).map_or(128, |b| be(b, 4) & 0x7fff_ffff);
                reader.chunk_size = (size as usize).clamp(1, 16 << 20);
            }
            5 => {
                window = body.get(..4).map(|b| u64::from(be(b, 4)));
            }
            6 => {
                // Set Peer Bandwidth: answered with the window it names.
                if let Some(size) = body.get(..4) {
                    let reply = chunks(2, 5, 0, 0, size, out_chunk);
                    write_half.write_all(&reply).await?;
                }
            }
            4 => {
                // A ping is answered with its own time.
                if body.get(..2) == Some(&[0, 6]) {
                    let mut pong = vec![0, 7];
                    pong.extend_from_slice(body.get(2..6).unwrap_or(&[0, 0, 0, 0]));
                    write_half
                        .write_all(&chunks(2, 4, 0, 0, &pong, out_chunk))
                        .await?;
                }
            }
            17 | 20 => {
                let values = amf_values(if message.kind == 17 {
                    body.get(1..).unwrap_or_default()
                } else {
                    body
                });
                let name = values.first().and_then(Amf::text).unwrap_or("");
                let transaction = match values.get(1) {
                    Some(Amf::Number(n)) => *n,
                    _ => 0.0,
                };
                match (name, transaction as u32) {
                    ("_result", 1) => {
                        let create =
                            command("createStream", 2.0, &[&|out: &mut Vec<u8>| out.push(0x05)]);
                        write_half
                            .write_all(&chunks(3, 20, 0, 0, &create, out_chunk))
                            .await?;
                    }
                    ("_result", 2) => {
                        let stream = match values.get(3) {
                            Some(Amf::Number(n)) => *n as u32,
                            _ => 1,
                        };
                        let mut buffer = vec![0, 3];
                        buffer.extend_from_slice(&stream.to_be_bytes());
                        buffer.extend_from_slice(&3000u32.to_be_bytes());
                        let play = command(
                            "play",
                            0.0,
                            &[
                                &|out: &mut Vec<u8>| out.push(0x05),
                                &|out: &mut Vec<u8>| amf_string(out, &target.play),
                                &|out: &mut Vec<u8>| amf_number(out, -2.0),
                            ],
                        );
                        let mut send = chunks(8, 20, stream, 0, &play, out_chunk);
                        send.extend(chunks(2, 4, 0, 0, &buffer, out_chunk));
                        write_half.write_all(&send).await?;
                    }
                    ("_error", _) => {
                        let why = values
                            .iter()
                            .find_map(|v| v.field("description").and_then(Amf::text))
                            .unwrap_or("the server refused")
                            .to_string();
                        break Ended::Stopped(Error::Internal(why));
                    }
                    ("onStatus", _) => {
                        let info = values.iter().find(|v| v.field("code").is_some());
                        let code = info
                            .and_then(|v| v.field("code"))
                            .and_then(Amf::text)
                            .unwrap_or("");
                        let level = info
                            .and_then(|v| v.field("level"))
                            .and_then(Amf::text)
                            .unwrap_or("");
                        match code {
                            "NetStream.Play.Start" | "NetStream.Play.Reset" => playing = true,
                            "NetStream.Play.Stop"
                            | "NetStream.Play.Complete"
                            | "NetStream.Play.UnpublishNotify" => break Ended::Over,
                            _ if level == "error" => {
                                let why = info
                                    .and_then(|v| v.field("description"))
                                    .and_then(Amf::text)
                                    .unwrap_or(code);
                                break Ended::Stopped(Error::Internal(format!(
                                    "the server would not play it: {why}"
                                )));
                            }
                            _ => {}
                        }
                    }
                    ("close", _) => break Ended::Over,
                    _ => {}
                }
            }
            8 => recorder.audio(message.timestamp, body)?,
            9 => recorder.video(message.timestamp, body)?,
            22 => {
                // Aggregate: FLV tags one after another.
                let mut at = 0;
                let mut base = None;
                while let Some(head) = body.get(at..at + 11) {
                    let size = be(head.get(1..).unwrap_or_default(), 3) as usize;
                    // Three bytes of time, then the top eight bits.
                    let time = be(head.get(4..).unwrap_or_default(), 3)
                        | (u32::from(head.get(7).copied().unwrap_or(0)) << 24);
                    let Some(data) = body.get(at + 11..at + 11 + size) else {
                        break;
                    };
                    let first = *base.get_or_insert(time);
                    let ms = message.timestamp.wrapping_add(time.wrapping_sub(first));
                    match head.first().copied().unwrap_or(0) & 0x1f {
                        8 => recorder.audio(ms, data)?,
                        9 => recorder.video(ms, data)?,
                        _ => {}
                    }
                    at += 11 + size + 4;
                }
            }
            _ => {}
        }
        let _ = playing;
        downloaded.store(recorder.written(), Ordering::Relaxed);
    };
    let written = recorder.finish()?;
    match (ended, written) {
        (Ended::Stopped(stop), _) => Err(stop),
        (Ended::Over, Some(bytes)) => Ok(bytes),
        (Ended::Over, None) => Err(Error::Internal(
            "the server ended the stream before sending any of it".into(),
        )),
    }
}
