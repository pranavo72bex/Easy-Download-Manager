//! RTMP chunks and messages, read and written.

use super::*;

/// One message as chunks of `chunk_size`, on chunk stream `csid`.
pub(super) fn chunks(
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
pub(super) type Arg<'a> = &'a dyn Fn(&mut Vec<u8>);

pub(super) fn command(name: &str, transaction: f64, args: &[Arg]) -> Vec<u8> {
    let mut out = Vec::new();
    amf_string(&mut out, name);
    amf_number(&mut out, transaction);
    for arg in args {
        arg(&mut out);
    }
    out
}

#[derive(Default)]
pub(super) struct ChunkState {
    pub(super) timestamp: u32,
    pub(super) delta: u32,
    pub(super) length: usize,
    pub(super) kind: u8,
    pub(super) stream: u32,
    pub(super) extended: bool,
    pub(super) body: Vec<u8>,
}

/// A whole message off the chunk stream.
pub(super) struct Message {
    pub(super) kind: u8,
    pub(super) timestamp: u32,
    pub(super) body: Vec<u8>,
}

pub(super) struct Reader<R> {
    pub(super) io: R,
    pub(super) chunk_size: usize,
    pub(super) streams: HashMap<u32, ChunkState>,
    pub(super) read: u64,
}

impl<R: AsyncRead + Unpin> Reader<R> {
    pub(super) async fn byte(&mut self) -> std::io::Result<u8> {
        self.read += 1;
        self.io.read_u8().await
    }

    pub(super) async fn bytes(&mut self, n: usize) -> std::io::Result<Vec<u8>> {
        let mut buf = vec![0u8; n];
        self.io.read_exact(&mut buf).await?;
        self.read += n as u64;
        Ok(buf)
    }

    /// Reads chunks until one completes a message.
    pub(super) async fn message(&mut self) -> Result<Message, Error> {
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
