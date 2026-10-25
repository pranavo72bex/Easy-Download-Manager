//! Turning what a stream sends into an MP4 as it arrives.

use super::*;

#[derive(Debug, Clone)]
pub(super) enum PictureConfig {
    Avc(Vec<u8>),
    Hevc(Vec<u8>),
}

#[derive(Debug, Clone, Copy)]
pub(super) enum SoundConfig {
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

pub(super) struct Held {
    pub(super) video: bool,
    pub(super) data: Vec<u8>,
    pub(super) ms: u32,
    pub(super) cts: i32,
    pub(super) key: bool,
}

/// What a stream's FLV tags become: samples in a fragmented MP4, once both
/// kinds' shapes are known (or a few seconds show one kind never comes).
pub(super) struct Recorder {
    pub(super) picture: Option<PictureConfig>,
    pub(super) aac_config: Option<Vec<u8>>,
    pub(super) sound: Option<SoundConfig>,
    pub(super) held: Vec<Held>,
    pub(super) first_ms: Option<u32>,
    pub(super) saw_video: bool,
    pub(super) saw_audio: bool,
    pub(super) writer: Option<fmp4::Writer>,
    pub(super) video_track: Option<usize>,
    pub(super) audio_track: Option<usize>,
    pub(super) path: std::path::PathBuf,
}

impl Recorder {
    pub(super) fn new(path: &Path) -> Recorder {
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

    pub(super) fn video(&mut self, ms: u32, data: &[u8]) -> std::io::Result<()> {
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

    pub(super) fn audio(&mut self, ms: u32, data: &[u8]) -> std::io::Result<()> {
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

    pub(super) fn take(&mut self, held: Held) -> std::io::Result<()> {
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
    pub(super) fn start_if_ready(&mut self, ms: u32) -> std::io::Result<()> {
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

    pub(super) fn write(&mut self, held: Held) -> std::io::Result<()> {
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

    pub(super) fn written(&self) -> u64 {
        self.writer.as_ref().map_or(0, |w| w.written)
    }

    /// Ends the file; `None` where nothing was recorded.
    pub(super) fn finish(mut self) -> std::io::Result<Option<u64>> {
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
pub(super) fn avc_size(record: &[u8]) -> (u32, u32) {
    let sps = record.get(6..8).and_then(|l| {
        let len = usize::from(be(l, 2) as u16);
        record.get(8..8 + len)
    });
    sps.and_then(crate::tsmux::picture_size).unwrap_or((0, 0))
}

/// A picture's size from the SPS in its `hvcC` record.
pub(super) fn hevc_size(record: &[u8]) -> (u32, u32) {
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
