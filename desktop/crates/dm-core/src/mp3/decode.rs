//! A sound file read to 16-bit samples by the system's own decoder, for the
//! MP3 encoder: on macOS AudioToolbox's ExtAudioFile, which reads AAC in an
//! M4A or MP4 (and turns another sample rate into one MP3 has). Elsewhere
//! there is none dm can use yet, and that is said in words.

use std::path::Path;

/// Interleaved 16-bit samples, their channels and their rate.
pub struct Pcm {
    pub samples: Vec<i16>,
    pub channels: usize,
    pub rate: u32,
}

/// `path`'s sound as samples, at most two channels, at a rate MP3 has.
pub fn read(path: &Path) -> Result<Pcm, String> {
    #[cfg(target_os = "macos")]
    {
        mac::read(path)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        Err(
            "saving as MP3 needs the system's sound decoder, which dm uses on macOS only so far"
                .into(),
        )
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use super::Pcm;
    use std::ffi::c_void;
    use std::path::Path;

    type OsStatus = i32;
    type ExtAudioFileRef = *mut c_void;
    type CfUrlRef = *const c_void;

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct StreamDescription {
        sample_rate: f64,
        format_id: u32,
        format_flags: u32,
        bytes_per_packet: u32,
        frames_per_packet: u32,
        bytes_per_frame: u32,
        channels_per_frame: u32,
        bits_per_channel: u32,
        reserved: u32,
    }

    #[repr(C)]
    struct AudioBuffer {
        channels: u32,
        byte_size: u32,
        data: *mut c_void,
    }

    #[repr(C)]
    struct AudioBufferList {
        count: u32,
        buffers: [AudioBuffer; 1],
    }

    #[link(name = "AudioToolbox", kind = "framework")]
    extern "C" {
        fn ExtAudioFileOpenURL(url: CfUrlRef, file: *mut ExtAudioFileRef) -> OsStatus;
        fn ExtAudioFileGetProperty(
            file: ExtAudioFileRef,
            id: u32,
            size: *mut u32,
            data: *mut c_void,
        ) -> OsStatus;
        fn ExtAudioFileSetProperty(
            file: ExtAudioFileRef,
            id: u32,
            size: u32,
            data: *const c_void,
        ) -> OsStatus;
        fn ExtAudioFileRead(
            file: ExtAudioFileRef,
            frames: *mut u32,
            buffers: *mut AudioBufferList,
        ) -> OsStatus;
        fn ExtAudioFileDispose(file: ExtAudioFileRef) -> OsStatus;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFURLCreateFromFileSystemRepresentation(
            allocator: *const c_void,
            path: *const u8,
            length: isize,
            is_directory: u8,
        ) -> CfUrlRef;
        fn CFRelease(object: *const c_void);
    }

    const fn four(code: &[u8; 4]) -> u32 {
        u32::from_be_bytes(*code)
    }

    pub(super) fn read(path: &Path) -> Result<Pcm, String> {
        use std::os::unix::ffi::OsStrExt;
        let bytes = path.as_os_str().as_bytes();
        let failed = |what: &str, status: OsStatus| {
            format!("the sound could not be read ({what}, {status})")
        };
        // SAFETY: the URL is made from a byte slice of its stated length
        // and released below; the file reference is used only after a
        // successful open and disposed of on every path; each property is
        // read into or written from a value of the size given; the buffer
        // list points into `chunk`, which outlives each read.
        unsafe {
            let url = CFURLCreateFromFileSystemRepresentation(
                std::ptr::null(),
                bytes.as_ptr(),
                bytes.len() as isize,
                0,
            );
            if url.is_null() {
                return Err("the sound's path could not be read".into());
            }
            let mut file: ExtAudioFileRef = std::ptr::null_mut();
            let opened = ExtAudioFileOpenURL(url, &mut file);
            CFRelease(url);
            if opened != 0 || file.is_null() {
                return Err(failed("open", opened));
            }
            let result = (|| {
                let mut source = StreamDescription::default();
                let mut size = std::mem::size_of::<StreamDescription>() as u32;
                let status = ExtAudioFileGetProperty(
                    file,
                    four(b"ffmt"),
                    &mut size,
                    (&mut source as *mut StreamDescription).cast(),
                );
                if status != 0 {
                    return Err(failed("format", status));
                }
                let channels = (source.channels_per_frame.clamp(1, 2)) as usize;
                let rate = match source.sample_rate as u32 {
                    r @ (32000 | 44100 | 48000) => r,
                    _ => 44100,
                };
                let client = StreamDescription {
                    sample_rate: f64::from(rate),
                    format_id: four(b"lpcm"),
                    // Signed integers, packed.
                    format_flags: 0x4 | 0x8,
                    bytes_per_packet: 2 * channels as u32,
                    frames_per_packet: 1,
                    bytes_per_frame: 2 * channels as u32,
                    channels_per_frame: channels as u32,
                    bits_per_channel: 16,
                    reserved: 0,
                };
                let status = ExtAudioFileSetProperty(
                    file,
                    four(b"cfmt"),
                    std::mem::size_of::<StreamDescription>() as u32,
                    (&client as *const StreamDescription).cast(),
                );
                if status != 0 {
                    return Err(failed("conversion", status));
                }
                let mut samples = Vec::new();
                let mut chunk = vec![0i16; 4096 * channels];
                loop {
                    let mut frames = 4096u32;
                    let mut list = AudioBufferList {
                        count: 1,
                        buffers: [AudioBuffer {
                            channels: channels as u32,
                            byte_size: (chunk.len() * 2) as u32,
                            data: chunk.as_mut_ptr().cast(),
                        }],
                    };
                    let status = ExtAudioFileRead(file, &mut frames, &mut list);
                    if status != 0 {
                        return Err(failed("read", status));
                    }
                    if frames == 0 {
                        break;
                    }
                    samples.extend_from_slice(&chunk[..frames as usize * channels]);
                }
                Ok(Pcm {
                    samples,
                    channels,
                    rate,
                })
            })();
            ExtAudioFileDispose(file);
            result
        }
    }
}
