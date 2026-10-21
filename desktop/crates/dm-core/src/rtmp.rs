//! An RTMP client of dm's own, for `rtmp://` and `rtmps://` streams.
//!
//! What a player does to watch one: the handshake, then `connect` to the
//! application, `createStream` and `play` the stream's name, in AMF0
//! commands over RTMP's chunk stream; the server's chunk size, window and
//! pings answered as it asks. Pictures (H.264, and HEVC as either RTMP
//! extension carries it) and sound (AAC, MP3) arrive as FLV tags and are
//! written as they come into a fragmented MP4 (`fmp4`), which plays however
//! far it got. RTMPE (Adobe's encryption) and RTMPT (HTTP tunnelling) are
//! not spoken here.
//!
//! Everything read comes from a server: nothing here indexes past what it
//! checked.

#![cfg_attr(not(test), deny(clippy::indexing_slicing))]

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

use crate::control::Control;
use crate::fmp4;
use crate::Error;

/// The parts of an `rtmp://` address a player uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub host: String,
    pub port: u16,
    pub tls: bool,
    /// The application: every part of the path but the last.
    pub app: String,
    /// The stream's name: the last part, with its query.
    pub play: String,
    pub tc_url: String,
}

/// Reads an `rtmp://` or `rtmps://` address.
pub fn target(url: &str) -> Result<Target, String> {
    let (scheme, rest) = url
        .trim()
        .split_once("://")
        .ok_or("not an rtmp:// address")?;
    let tls = match scheme.to_ascii_lowercase().as_str() {
        "rtmp" => false,
        "rtmps" => true,
        other => return Err(format!("dm does not speak {other}://")),
    };
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !host.ends_with(']') || authority.starts_with('[') => {
            match port.parse() {
                Ok(port) => (host, port),
                Err(_) => (authority, if tls { 443 } else { 1935 }),
            }
        }
        _ => (authority, if tls { 443 } else { 1935 }),
    };
    let host = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();
    if host.is_empty() {
        return Err("the address names no server".into());
    }
    let (path, query) = match path.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (path, None),
    };
    let path = path.trim_end_matches('/');
    let (app, play) = match path.rsplit_once('/') {
        Some((app, play)) => (app.to_string(), play.to_string()),
        None => (path.to_string(), String::new()),
    };
    let play = match query {
        Some(q) if !play.is_empty() => format!("{play}?{q}"),
        _ => play,
    };
    if app.is_empty() || play.is_empty() {
        return Err("the address names no application and stream".into());
    }
    let tc_url = format!(
        "{}://{}:{port}/{app}",
        if tls { "rtmps" } else { "rtmp" },
        if host.contains(':') {
            format!("[{host}]")
        } else {
            host.clone()
        }
    );
    Ok(Target {
        host,
        port,
        tls,
        app,
        play,
        tc_url,
    })
}

/// The big-endian integer in the first `n` bytes of `b`; zero where `b`
/// is shorter.
fn be(b: &[u8], n: usize) -> u32 {
    b.get(..n)
        .map_or(0, |v| v.iter().fold(0u32, |a, x| (a << 8) | u32::from(*x)))
}

// ---- AMF0 ---------------------------------------------------------------

/// An AMF0 value, as much of one as a player reads.
#[derive(Debug, Clone, PartialEq)]
pub enum Amf {
    Number(f64),
    Bool(bool),
    String(String),
    Object(Vec<(String, Amf)>),
    Null,
    Other,
}

impl Amf {
    fn field(&self, name: &str) -> Option<&Amf> {
        match self {
            Amf::Object(fields) => fields.iter().find(|(k, _)| k == name).map(|(_, v)| v),
            _ => None,
        }
    }

    fn text(&self) -> Option<&str> {
        match self {
            Amf::String(s) => Some(s),
            _ => None,
        }
    }
}

fn amf_string(out: &mut Vec<u8>, s: &str) {
    out.push(0x02);
    out.extend_from_slice(&(s.len().min(0xFFFF) as u16).to_be_bytes());
    out.extend_from_slice(s.as_bytes().get(..s.len().min(0xFFFF)).unwrap_or_default());
}

fn amf_number(out: &mut Vec<u8>, n: f64) {
    out.push(0x00);
    out.extend_from_slice(&n.to_be_bytes());
}

fn amf_object(out: &mut Vec<u8>, fields: &[(&str, Amf)]) {
    out.push(0x03);
    for (key, value) in fields {
        out.extend_from_slice(&(key.len() as u16).to_be_bytes());
        out.extend_from_slice(key.as_bytes());
        match value {
            Amf::Number(n) => amf_number(out, *n),
            Amf::Bool(b) => out.extend_from_slice(&[0x01, u8::from(*b)]),
            Amf::String(s) => amf_string(out, s),
            _ => out.push(0x05),
        }
    }
    out.extend_from_slice(&[0, 0, 0x09]);
}

/// The AMF0 values in `data`, as many as can be read.
pub fn amf_values(data: &[u8]) -> Vec<Amf> {
    let mut at = 0;
    let mut values = Vec::new();
    while at < data.len() {
        match amf_value(data, &mut at, 0) {
            Some(v) => values.push(v),
            None => break,
        }
    }
    values
}

fn amf_value(data: &[u8], at: &mut usize, depth: u32) -> Option<Amf> {
    if depth > 16 {
        return None;
    }
    let marker = *data.get(*at)?;
    *at += 1;
    let short_string = |at: &mut usize| -> Option<String> {
        let len = usize::from(u16::from_be_bytes([*data.get(*at)?, *data.get(*at + 1)?]));
        let text = data.get(*at + 2..*at + 2 + len)?;
        *at += 2 + len;
        Some(String::from_utf8_lossy(text).into_owned())
    };
    let fields = |at: &mut usize| -> Option<Vec<(String, Amf)>> {
        let mut fields = Vec::new();
        loop {
            if data.get(*at..*at + 3) == Some(&[0, 0, 0x09]) {
                *at += 3;
                return Some(fields);
            }
            let key = short_string(at)?;
            let value = amf_value(data, at, depth + 1)?;
            fields.push((key, value));
        }
    };
    Some(match marker {
        0x00 => {
            let n = f64::from_be_bytes(data.get(*at..*at + 8)?.try_into().ok()?);
            *at += 8;
            Amf::Number(n)
        }
        0x01 => {
            let b = *data.get(*at)? != 0;
            *at += 1;
            Amf::Bool(b)
        }
        0x02 => Amf::String(short_string(at)?),
        0x03 => Amf::Object(fields(at)?),
        0x05 | 0x06 => Amf::Null,
        0x08 => {
            *at += 4;
            Amf::Object(fields(at)?)
        }
        0x0A => {
            let count = u32::from_be_bytes(data.get(*at..*at + 4)?.try_into().ok()?);
            *at += 4;
            for _ in 0..count.min(4096) {
                amf_value(data, at, depth + 1)?;
            }
            Amf::Other
        }
        0x0B => {
            *at += 10;
            Amf::Other
        }
        0x0C => {
            let len = u32::from_be_bytes(data.get(*at..*at + 4)?.try_into().ok()?) as usize;
            let text = data.get(*at + 4..(*at + 4).checked_add(len)?)?;
            *at += 4 + len;
            Amf::String(String::from_utf8_lossy(text).into_owned())
        }
        _ => return None,
    })
}

// ---- the chunk stream ---------------------------------------------------

/// One message as chunks of `chunk_size`, on chunk stream `csid`.
fn chunks(
    csid: u8,
    kind: u8,
    stream: u32,
    timestamp: u32,
    payload: &[u8],
    chunk_size: usize,
) -> Vec<u8> {
    let mut out = vec![csid & 0x3f];
    out.extend_from_slice(&timestamp.min(0x00FF_FFFF).to_be_bytes()[1..]);
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes()[1..]);
    out.push(kind);
    out.extend_from_slice(&stream.to_le_bytes());
    for (i, part) in payload.chunks(chunk_size.max(1)).enumerate() {
        if i > 0 {
            out.push(0xC0 | (csid & 0x3f));
        }
        out.extend_from_slice(part);
    }
    out
}

/// One argument of a command, written onto its body.
type Arg<'a> = &'a dyn Fn(&mut Vec<u8>);

fn command(name: &str, transaction: f64, args: &[Arg]) -> Vec<u8> {
    let mut out = Vec::new();
    amf_string(&mut out, name);
    amf_number(&mut out, transaction);
    for arg in args {
        arg(&mut out);
    }
    out
}

#[derive(Default)]
struct ChunkState {
    timestamp: u32,
    delta: u32,
    length: usize,
    kind: u8,
    stream: u32,
    extended: bool,
    body: Vec<u8>,
}

/// A whole message off the chunk stream.
struct Message {
    kind: u8,
    timestamp: u32,
    body: Vec<u8>,
}

struct Reader<R> {
    io: R,
    chunk_size: usize,
    streams: HashMap<u32, ChunkState>,
    read: u64,
}

impl<R: AsyncRead + Unpin> Reader<R> {
    async fn byte(&mut self) -> std::io::Result<u8> {
        self.read += 1;
        self.io.read_u8().await
    }

    async fn bytes(&mut self, n: usize) -> std::io::Result<Vec<u8>> {
        let mut buf = vec![0u8; n];
        self.io.read_exact(&mut buf).await?;
        self.read += n as u64;
        Ok(buf)
    }

    /// Reads chunks until one completes a message.
    async fn message(&mut self) -> Result<Message, Error> {
        loop {
            let first = self.byte().await?;
            let fmt = first >> 6;
            let csid = match first & 0x3f {
                0 => 64 + u32::from(self.byte().await?),
                1 => {
                    let b = self.bytes(2).await?;
                    64 + u32::from(b.first().copied().unwrap_or(0))
                        + 256 * u32::from(b.get(1).copied().unwrap_or(0))
                }
                n => u32::from(n),
            };
            let header = self
                .bytes(match fmt {
                    0 => 11,
                    1 => 7,
                    2 => 3,
                    _ => 0,
                })
                .await?;
            let u24 = |b: &[u8], at: usize| b.get(at..).map_or(0, |v| be(v, 3));
            let state = self.streams.entry(csid).or_default();
            let starting = state.body.is_empty();
            let mut time_field = None;
            match fmt {
                0 => {
                    time_field = Some(u24(&header, 0));
                    state.length = u24(&header, 3) as usize;
                    state.kind = header.get(6).copied().unwrap_or(0);
                    state.stream = header
                        .get(7..11)
                        .and_then(|b| b.try_into().ok())
                        .map_or(0, u32::from_le_bytes);
                }
                1 => {
                    time_field = Some(u24(&header, 0));
                    state.length = u24(&header, 3) as usize;
                    state.kind = header.get(6).copied().unwrap_or(0);
                }
                2 => time_field = Some(u24(&header, 0)),
                _ => {}
            }
            if let Some(t) = time_field {
                state.extended = t == 0x00FF_FFFF;
            }
            let extended = state.extended;
            let ext = if extended {
                let b = self.bytes(4).await?;
                Some(u32::from_be_bytes(b.try_into().unwrap_or([0; 4])))
            } else {
                None
            };
            let state = self.streams.entry(csid).or_default();
            if starting {
                let value = ext.or(time_field);
                match (fmt, value) {
                    (0, Some(t)) => {
                        state.timestamp = t;
                        state.delta = 0;
                    }
                    (1 | 2, Some(d)) => {
                        state.delta = d;
                        state.timestamp = state.timestamp.wrapping_add(d);
                    }
                    _ => state.timestamp = state.timestamp.wrapping_add(state.delta),
                }
            }
            if state.length > 16 << 20 {
                return Err(Error::Internal(
                    "the server sent a message too large".into(),
                ));
            }
            let want = (state.length - state.body.len().min(state.length)).min(self.chunk_size);
            let part = {
                let mut buf = vec![0u8; want];
                self.io.read_exact(&mut buf).await?;
                buf
            };
            self.read += want as u64;
            let state = self.streams.entry(csid).or_default();
            state.body.extend_from_slice(&part);
            if state.body.len() >= state.length {
                let body = std::mem::take(&mut state.body);
                return Ok(Message {
                    kind: state.kind,
                    timestamp: state.timestamp,
                    body,
                });
            }
        }
    }
}

// ---- what is recorded ---------------------------------------------------

#[derive(Debug, Clone)]
enum PictureConfig {
    Avc(Vec<u8>),
    Hevc(Vec<u8>),
}

#[derive(Debug, Clone, Copy)]
enum SoundConfig {
    Aac {
        rate: u32,
        channels: u16,
    },
    Mp3 {
        rate: u32,
        channels: u16,
        per_frame: u32,
        mpeg1: bool,
    },
}

struct Held {
    video: bool,
    data: Vec<u8>,
    ms: u32,
    cts: i32,
    key: bool,
}

/// What a stream's FLV tags become: samples in a fragmented MP4, once both
/// kinds' shapes are known (or a few seconds show one kind never comes).
struct Recorder {
    picture: Option<PictureConfig>,
    aac_config: Option<Vec<u8>>,
    sound: Option<SoundConfig>,
    held: Vec<Held>,
    first_ms: Option<u32>,
    saw_video: bool,
    saw_audio: bool,
    writer: Option<fmp4::Writer>,
    video_track: Option<usize>,
    audio_track: Option<usize>,
    path: std::path::PathBuf,
}

impl Recorder {
    fn new(path: &Path) -> Recorder {
        Recorder {
            picture: None,
            aac_config: None,
            sound: None,
            held: Vec::new(),
            first_ms: None,
            saw_video: false,
            saw_audio: false,
            writer: None,
            video_track: None,
            audio_track: None,
            path: path.to_path_buf(),
        }
    }

    fn video(&mut self, ms: u32, data: &[u8]) -> std::io::Result<()> {
        let Some(&head) = data.first() else {
            return Ok(());
        };
        let i24 = |b: &[u8]| -> i32 {
            let v = be(b, 3) as i32;
            (v << 8) >> 8
        };
        let (key, config, frame, cts) = if head & 0x80 != 0 {
            // Enhanced RTMP: a packet type and the codec's FourCC.
            let kind = head & 0x0f;
            let key = (head >> 4) & 0x07 == 1;
            let fourcc = data.get(1..5).unwrap_or_default();
            let hevc = fourcc == b"hvc1";
            if !(hevc || fourcc == b"avc1") {
                return Ok(());
            }
            let rest = data.get(5..).unwrap_or_default();
            match kind {
                0 => (key, Some((hevc, rest)), None, 0),
                1 => (key, None, rest.get(3..), i24(rest)),
                3 => (key, None, Some(rest), 0),
                _ => return Ok(()),
            }
        } else {
            let codec = head & 0x0f;
            if codec != 7 && codec != 12 {
                return Ok(());
            }
            let key = head >> 4 == 1;
            let packet = data.get(1).copied().unwrap_or(0);
            let cts = i24(data.get(2..).unwrap_or_default());
            let rest = data.get(5..).unwrap_or_default();
            match packet {
                0 => (key, Some((codec == 12, rest)), None, 0),
                1 => (key, None, Some(rest), cts),
                _ => return Ok(()),
            }
        };
        if let Some((hevc, record)) = config {
            if self.picture.is_none() && !record.is_empty() {
                self.picture = Some(if hevc {
                    PictureConfig::Hevc(record.to_vec())
                } else {
                    PictureConfig::Avc(record.to_vec())
                });
            }
            return self.start_if_ready(ms);
        }
        let Some(frame) = frame.filter(|f| !f.is_empty()) else {
            return Ok(());
        };
        self.saw_video = true;
        self.take(Held {
            video: true,
            data: frame.to_vec(),
            ms,
            cts,
            key,
        })
    }

    fn audio(&mut self, ms: u32, data: &[u8]) -> std::io::Result<()> {
        let Some(&head) = data.first() else {
            return Ok(());
        };
        match head >> 4 {
            10 => {
                let rest = data.get(2..).unwrap_or_default();
                if data.get(1) == Some(&0) {
                    if self.aac_config.is_none() && rest.len() >= 2 {
                        let (rate, channels) = fmp4::aac_shape(rest);
                        self.aac_config = Some(rest.to_vec());
                        self.sound = Some(SoundConfig::Aac { rate, channels });
                    }
                    return self.start_if_ready(ms);
                }
                if rest.is_empty() {
                    return Ok(());
                }
                self.saw_audio = true;
                self.take(Held {
                    video: false,
                    data: rest.to_vec(),
                    ms,
                    cts: 0,
                    key: true,
                })
            }
            2 => {
                let frame = data.get(1..).unwrap_or_default();
                if self.sound.is_none() {
                    if let crate::tsmux::Frame::Whole {
                        shape:
                            crate::tsmux::Sound::Mpeg {
                                mpeg1,
                                rate,
                                channels,
                                per_frame,
                            },
                        ..
                    } = crate::tsmux::mpeg_audio_frame(frame)
                    {
                        self.sound = Some(SoundConfig::Mp3 {
                            rate,
                            channels,
                            per_frame,
                            mpeg1,
                        });
                    }
                }
                if frame.is_empty() {
                    return Ok(());
                }
                self.saw_audio = true;
                self.take(Held {
                    video: false,
                    data: frame.to_vec(),
                    ms,
                    cts: 0,
                    key: true,
                })
            }
            _ => Ok(()),
        }
    }

    fn take(&mut self, held: Held) -> std::io::Result<()> {
        let ms = held.ms;
        if self.writer.is_some() {
            return self.write(held);
        }
        if self.held.len() < 20_000 {
            self.held.push(held);
        }
        self.start_if_ready(ms)
    }

    /// Starts the file once every kind seen has its shape, or three
    /// seconds in with what has one.
    fn start_if_ready(&mut self, ms: u32) -> std::io::Result<()> {
        if self.writer.is_some() {
            return Ok(());
        }
        let first = *self.first_ms.get_or_insert(ms);
        let late = ms.saturating_sub(first) >= 3000;
        let picture_ready = self.picture.is_some() || (!self.saw_video && late);
        let sound_ready = self.sound.is_some() || (!self.saw_audio && late);
        if !(picture_ready && sound_ready) && !late {
            return Ok(());
        }
        if self.picture.is_none() && self.sound.is_none() {
            return Ok(());
        }
        let mut specs = Vec::new();
        if let Some(picture) = &self.picture {
            let (entry, width, height) = match picture {
                PictureConfig::Avc(record) => {
                    let (w, h) = avc_size(record);
                    (fmp4::avc1(record, w, h), w, h)
                }
                PictureConfig::Hevc(record) => {
                    let (w, h) = hevc_size(record);
                    (fmp4::hvc1(record, w, h), w, h)
                }
            };
            self.video_track = Some(specs.len());
            specs.push(fmp4::Spec {
                entry,
                timescale: 1000,
                video: true,
                width,
                height,
            });
        }
        if let Some(sound) = self.sound {
            let (entry, rate) = match sound {
                SoundConfig::Aac { rate, channels } => (
                    fmp4::mp4a(0x40, self.aac_config.as_deref(), rate, channels),
                    rate,
                ),
                SoundConfig::Mp3 {
                    rate,
                    channels,
                    mpeg1,
                    ..
                } => (
                    fmp4::mp4a(if mpeg1 { 0x6b } else { 0x69 }, None, rate, channels),
                    rate,
                ),
            };
            self.audio_track = Some(specs.len());
            specs.push(fmp4::Spec {
                entry,
                timescale: rate,
                video: false,
                width: 0,
                height: 0,
            });
        }
        self.writer = Some(fmp4::Writer::create(&self.path, specs)?);
        for held in std::mem::take(&mut self.held) {
            self.write(held)?;
        }
        Ok(())
    }

    fn write(&mut self, held: Held) -> std::io::Result<()> {
        let Some(writer) = self.writer.as_mut() else {
            return Ok(());
        };
        if held.video {
            if let Some(track) = self.video_track {
                writer.push(
                    track,
                    held.data,
                    i64::from(held.ms),
                    held.cts,
                    held.key,
                    None,
                )?;
            }
        } else if let (Some(track), Some(sound)) = (self.audio_track, self.sound) {
            let (rate, per_frame) = match sound {
                SoundConfig::Aac { rate, .. } => (rate, 1024),
                SoundConfig::Mp3 {
                    rate, per_frame, ..
                } => (rate, per_frame),
            };
            let dts = i64::from(held.ms) * i64::from(rate) / 1000;
            writer.push(track, held.data, dts, 0, true, Some(per_frame))?;
        }
        Ok(())
    }

    fn written(&self) -> u64 {
        self.writer.as_ref().map_or(0, |w| w.written)
    }

    /// Ends the file; `None` where nothing was recorded.
    fn finish(mut self) -> std::io::Result<Option<u64>> {
        if self.writer.is_none() && !self.held.is_empty() {
            self.start_if_ready(u32::MAX)?;
        }
        match self.writer {
            Some(writer) => writer.finish().map(Some),
            None => Ok(None),
        }
    }
}

/// A picture's size from the SPS in its `avcC` record.
fn avc_size(record: &[u8]) -> (u32, u32) {
    let sps = record.get(6..8).and_then(|l| {
        let len = usize::from(be(l, 2) as u16);
        record.get(8..8 + len)
    });
    sps.and_then(crate::tsmux::picture_size).unwrap_or((0, 0))
}

/// A picture's size from the SPS in its `hvcC` record.
fn hevc_size(record: &[u8]) -> (u32, u32) {
    let mut at = 23;
    for _ in 0..record.get(22).copied().unwrap_or(0) {
        let Some(kind) = record.get(at).map(|k| k & 0x3f) else {
            break;
        };
        let count = record.get(at + 1..at + 3).map_or(0, |b| be(b, 2) as u16);
        at += 3;
        for _ in 0..count {
            let len = record.get(at..at + 2).map_or(0, |b| be(b, 2) as usize);
            let unit = record.get(at + 2..at + 2 + len);
            at += 2 + len;
            if kind == 33 {
                if let Some(info) = unit.and_then(crate::tsmux::hevc_sps) {
                    return (info.width, info.height);
                }
            }
        }
    }
    (0, 0)
}

// ---- the session --------------------------------------------------------

trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

async fn open(target: &Target) -> Result<Box<dyn Io>, Error> {
    let tcp = tokio::time::timeout(
        Duration::from_secs(15),
        tokio::net::TcpStream::connect((target.host.as_str(), target.port)),
    )
    .await
    .map_err(|_| Error::Internal(format!("{} did not answer", target.host)))??;
    let _ = tcp.set_nodelay(true);
    if !target.tls {
        return Ok(Box::new(tcp));
    }
    let mut roots = tokio_rustls::rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    // The provider named: two are compiled in (through other crates), and
    // rustls will not pick between them.
    let config = tokio_rustls::rustls::ClientConfig::builder_with_provider(Arc::new(
        tokio_rustls::rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|e| Error::Internal(format!("TLS: {e}")))?
    .with_root_certificates(roots)
    .with_no_client_auth();
    let name = tokio_rustls::rustls::pki_types::ServerName::try_from(target.host.clone())
        .map_err(|e| Error::Internal(format!("{}: {e}", target.host)))?;
    let tls = tokio_rustls::TlsConnector::from(Arc::new(config))
        .connect(name, tcp)
        .await?;
    Ok(Box::new(tls))
}

/// The simple handshake: C0 and C1 out, S0, S1 and S2 in, C2 (S1 back).
async fn handshake(io: &mut dyn Io) -> Result<(), Error> {
    let mut hello = vec![3u8];
    hello.extend_from_slice(&[0u8; 8]);
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1, |d| d.as_nanos() as u64)
        | 1;
    for _ in 0..1528 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        hello.push(seed as u8);
    }
    io.write_all(&hello).await?;
    let mut s0s1 = vec![0u8; 1537];
    io.read_exact(&mut s0s1).await?;
    if s0s1.first() != Some(&3) {
        return Err(Error::Internal(
            "the server does not speak plain RTMP".into(),
        ));
    }
    io.write_all(s0s1.get(1..).unwrap_or_default()).await?;
    let mut s2 = vec![0u8; 1536];
    io.read_exact(&mut s2).await?;
    Ok(())
}

/// How a session ended.
enum Ended {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_read_as_a_player_reads_them() {
        let t = target("rtmp://media.example/live/show").unwrap();
        assert_eq!(
            (t.host.as_str(), t.port, t.tls),
            ("media.example", 1935, false)
        );
        assert_eq!((t.app.as_str(), t.play.as_str()), ("live", "show"));
        assert_eq!(t.tc_url, "rtmp://media.example:1935/live");
        let t = target("rtmps://media.example:8443/app/inst/name?key=1").unwrap();
        assert_eq!((t.port, t.tls), (8443, true));
        assert_eq!(
            (t.app.as_str(), t.play.as_str()),
            ("app/inst", "name?key=1")
        );
        assert!(target("rtmp://media.example/").is_err());
        assert!(target("rtmpe://media.example/live/x").is_err());
    }

    #[test]
    fn amf_round_trips() {
        let mut out = Vec::new();
        amf_string(&mut out, "onStatus");
        amf_number(&mut out, 0.0);
        out.push(0x05);
        amf_object(
            &mut out,
            &[
                ("level", Amf::String("error".into())),
                ("code", Amf::String("NetStream.Play.StreamNotFound".into())),
            ],
        );
        let values = amf_values(&out);
        assert_eq!(values.len(), 4);
        assert_eq!(values[0].text(), Some("onStatus"));
        assert_eq!(
            values[3].field("code").and_then(Amf::text),
            Some("NetStream.Play.StreamNotFound")
        );
    }

    #[test]
    fn a_message_goes_out_in_chunks() {
        let payload = vec![7u8; 300];
        let out = chunks(3, 20, 1, 0, &payload, 128);
        // A full header, then two continuation bytes between the parts.
        assert_eq!(out.len(), 12 + 300 + 2);
        assert_eq!(out[0], 3);
        assert_eq!(out[12 + 128], 0xC3);
    }

    proptest::proptest! {
        #[test]
        fn any_bytes_are_read_without_a_panic(data in proptest::collection::vec(proptest::num::u8::ANY, 0..600)) {
            let _ = amf_values(&data);
            let _ = avc_size(&data);
            let _ = hevc_size(&data);
            let dir = tempfile::tempdir().unwrap();
            let mut r = Recorder::new(&dir.path().join("x.mp4"));
            let _ = r.video(0, &data);
            let _ = r.audio(0, &data);
            let _ = r.finish();
        }
    }
}
