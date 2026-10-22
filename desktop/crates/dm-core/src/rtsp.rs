//! RTSP streams (`rtsp://`), recorded by dm itself.
//!
//! RTSP is what IP cameras and media servers speak. The session — DESCRIBE,
//! SETUP over the RTSP connection itself (RTP interleaved on TCP, which
//! passes NAT and firewalls), PLAY — and the depacketizing of H.264, H.265
//! and AAC are the `retina` crate's. Its frames, already in the form an MP4
//! keeps them, go into the same fragmented MP4 as RTMP's (`fmp4`), which
//! plays however far it got. A camera's other tracks (G.711 sound, ONVIF
//! metadata) are left out rather than failing the recording.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use retina::client::{
    Credentials, InitialTimestampPolicy, PlayOptions, Session, SessionOptions, SetupOptions,
    TcpTransportOptions, Transport,
};
use retina::codec::{CodecItem, FrameFormat, ParametersRef};

use crate::control::Control;
use crate::fmp4;
use crate::Error;

/// A frame waiting for the file to start: its stream (0 picture, 1 sound),
/// data, time in its clock, whether it is a key frame, and its length where
/// the source says.
type Frame = (usize, Vec<u8>, i64, bool, Option<u32>);

fn failed(e: impl std::fmt::Display) -> Error {
    Error::Internal(format!("the RTSP server: {e}"))
}

/// The address without its sign-in, and the sign-in: retina takes them
/// apart, and refuses an address that carries one.
fn split_credentials(url: &str) -> Result<(url::Url, Option<Credentials>), Error> {
    let mut parsed = url::Url::parse(url.trim()).map_err(|e| Error::Internal(e.to_string()))?;
    let creds = (!parsed.username().is_empty()).then(|| Credentials {
        username: crate::probe::decode(parsed.username()),
        password: crate::probe::decode(parsed.password().unwrap_or("")),
    });
    let _ = parsed.set_username("");
    let _ = parsed.set_password(None);
    Ok((parsed, creds))
}

/// A task that ends with the recording that needed it.
struct Aborted(tokio::task::JoinHandle<()>);

impl Drop for Aborted {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// The address retina is to use for an `rtsps://` one, and the task that
/// carries it: one connection taken on a loopback port, relayed over TLS
/// (checked against `config`) to the server, by default on port 322. The
/// server is asked for the loopback address; servers go by its path.
pub(crate) async fn bridge(
    address: &url::Url,
    config: std::sync::Arc<tokio_rustls::rustls::ClientConfig>,
) -> Result<(url::Url, tokio::task::JoinHandle<()>), Error> {
    let host = address
        .host_str()
        .ok_or_else(|| Error::Internal("the address names no server".into()))?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();
    let port = address.port().unwrap_or(322);
    let tcp = tokio::time::timeout(
        Duration::from_secs(15),
        tokio::net::TcpStream::connect((host.as_str(), port)),
    )
    .await
    .map_err(|_| Error::Internal(format!("{host} did not answer")))??;
    let name = tokio_rustls::rustls::pki_types::ServerName::try_from(host.clone())
        .map_err(|e| Error::Internal(format!("{host}: {e}")))?;
    let mut tls = tokio_rustls::TlsConnector::from(config)
        .connect(name, tcp)
        .await?;
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    let local_port = listener.local_addr()?.port();
    let task = tokio::spawn(async move {
        if let Ok((mut local, _)) = listener.accept().await {
            let _ = tokio::io::copy_bidirectional(&mut local, &mut tls).await;
        }
    });
    let mut local = address.clone();
    let rewritten = local.set_scheme("rtsp").is_ok()
        && local.set_host(Some("127.0.0.1")).is_ok()
        && local.set_port(Some(local_port)).is_ok();
    if !rewritten {
        task.abort();
        return Err(Error::Internal(
            "the rtsps:// address could not be read".into(),
        ));
    }
    Ok((local, task))
}

/// Records the stream at `url` into `part` until its server ends it, the
/// connection closes, or `control` stops it. Returns the bytes written; a
/// stop is returned after the file is closed, so what arrived plays.
pub async fn record(
    url: &str,
    part: &Path,
    control: &Control,
    downloaded: &Arc<AtomicU64>,
) -> Result<u64, Error> {
    let (mut address, creds) = split_credentials(url)?;
    // rtsps://: retina speaks plain RTSP only, so it is given a loopback
    // address that dm carries over TLS to the server. Held for the run.
    let _bridge = if address.scheme().eq_ignore_ascii_case("rtsps") {
        let (local, task) = bridge(&address, crate::rtmp::tls_config()?).await?;
        address = local;
        Some(Aborted(task))
    } else {
        None
    };
    let options = SessionOptions::default()
        .creds(creds)
        .user_agent("dm".into());
    let mut session =
        tokio::time::timeout(Duration::from_secs(20), Session::describe(address, options))
            .await
            .map_err(|_| Error::Internal("the RTSP server did not answer".into()))?
            .map_err(failed)?;

    // The first picture and the first sound dm can put into an MP4.
    let wanted = |media: &str, names: &[&str]| {
        session
            .streams()
            .iter()
            .position(|s| s.media() == media && names.contains(&s.encoding_name()))
    };
    let video = wanted("video", &["h264", "h265"]);
    let audio = wanted("audio", &["mpeg4-generic"]);
    if video.is_none() && audio.is_none() {
        return Err(Error::Internal(
            "the RTSP stream has no H.264, H.265 or AAC dm can record".into(),
        ));
    }
    for i in [video, audio].into_iter().flatten() {
        session
            .setup(
                i,
                SetupOptions::default()
                    .transport(Transport::Tcp(TcpTransportOptions::default()))
                    .frame_format(FrameFormat::MP4),
            )
            .await
            .map_err(failed)?;
    }
    // Permissive about the PLAY reply's timing: servers (mediamtx among
    // them) leave `rtptime` out, and the first packet's time serves.
    let playing = session
        .play(PlayOptions::default().initial_timestamp(InitialTimestampPolicy::Permissive))
        .await
        .map_err(failed)?;
    let mut frames = playing.demuxed().map_err(failed)?;

    let mut writer: Option<fmp4::Writer> = None;
    let mut tracks: [Option<usize>; 2] = [None, None];
    let mut held: Vec<Frame> = Vec::new();
    let started = std::time::Instant::now();
    let mut stop = None;
    loop {
        let next = tokio::select! {
            item = tokio::time::timeout(Duration::from_secs(30), frames.next()) => item,
            _ = tokio::time::sleep(Duration::from_millis(250)) => {
                if let Err(e) = control.check() {
                    stop = Some(e);
                    break;
                }
                continue;
            }
        };
        let item = match next {
            Ok(Some(Ok(item))) => item,
            Ok(Some(Err(e))) => {
                if writer.is_none() {
                    stop = Some(failed(e));
                }
                break;
            }
            Ok(None) => break,
            Err(_) => {
                if writer.is_none() {
                    stop = Some(Error::Internal(
                        "the RTSP server sent nothing for 30 seconds".into(),
                    ));
                }
                break;
            }
        };
        let frame: Frame = match item {
            CodecItem::VideoFrame(f) if Some(f.stream_id()) == video => {
                let key = f.is_random_access_point();
                let at = f.timestamp().elapsed();
                (0usize, f.into_data(), at, key, None)
            }
            CodecItem::AudioFrame(f) if Some(f.stream_id()) == audio => {
                let at = f.timestamp().elapsed();
                let length = f.frame_length().get();
                (1usize, f.data().to_vec(), at, true, Some(length))
            }
            _ => continue,
        };
        if writer.is_none() {
            held.push(frame);
            // Started once each stream's shape is known, or after three
            // seconds with those that are.
            let known = |i: Option<usize>| {
                i.is_none_or(|i| {
                    frames
                        .streams()
                        .get(i)
                        .and_then(|s| s.parameters())
                        .is_some()
                })
            };
            if !(known(video) && known(audio)) && started.elapsed() < Duration::from_secs(3) {
                continue;
            }
            let mut specs = Vec::new();
            for (slot, index) in [video, audio].into_iter().enumerate() {
                let Some(stream) = index.and_then(|i| frames.streams().get(i)) else {
                    continue;
                };
                let spec = match stream.parameters() {
                    Some(ParametersRef::Video(v)) => {
                        let (width, height) = v.pixel_dimensions();
                        v.mp4_sample_entry().build().ok().map(|entry| fmp4::Spec {
                            entry,
                            timescale: stream.clock_rate_hz(),
                            video: true,
                            width,
                            height,
                        })
                    }
                    Some(ParametersRef::Audio(a)) => {
                        a.mp4_sample_entry().build().ok().map(|entry| fmp4::Spec {
                            entry,
                            timescale: a.clock_rate(),
                            video: false,
                            width: 0,
                            height: 0,
                        })
                    }
                    _ => None,
                };
                if let (Some(spec), Some(t)) = (spec, tracks.get_mut(slot)) {
                    *t = Some(specs.len());
                    specs.push(spec);
                }
            }
            if specs.is_empty() {
                if started.elapsed() > Duration::from_secs(10) {
                    stop = Some(Error::Internal(
                        "the RTSP stream never said what its picture or sound is".into(),
                    ));
                    break;
                }
                continue;
            }
            writer = Some(fmp4::Writer::create(part, specs)?);
            for (slot, data, at, key, length) in std::mem::take(&mut held) {
                if let (Some(w), Some(Some(track))) = (writer.as_mut(), tracks.get(slot)) {
                    w.push(*track, data, at, 0, key, length)?;
                }
            }
        } else if let (Some(w), Some(Some(track))) = (writer.as_mut(), tracks.get(frame.0)) {
            w.push(*track, frame.1, frame.2, 0, frame.3, frame.4)?;
        }
        if let Some(w) = &writer {
            downloaded.store(w.written, Ordering::Relaxed);
        }
    }
    let written = match writer {
        Some(w) => Some(w.finish()?),
        None => None,
    };
    match (stop, written) {
        (Some(stop), _) => Err(stop),
        (None, Some(bytes)) => Ok(bytes),
        (None, None) => Err(Error::Internal(
            "the RTSP server ended the stream before sending any of it".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// rtsps://: retina is handed a loopback rtsp:// address with the
    /// path, query and port it needs, and what it sends there reaches the
    /// server over TLS, and the server's answer comes back.
    #[tokio::test]
    async fn an_rtsps_address_is_carried_over_tls() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio_rustls::rustls;
        let made = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let cert = made.cert.der().clone();
        let key =
            rustls::pki_types::PrivateKeyDer::try_from(made.key_pair.serialize_der()).unwrap();
        let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
        let server = rustls::ServerConfig::builder_with_provider(provider.clone())
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(vec![cert.clone()], key)
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let acceptor = tokio_rustls::TlsAcceptor::from(std::sync::Arc::new(server));
        let served = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut tls = acceptor.accept(tcp).await.unwrap();
            let mut asked = vec![0u8; 64];
            let n = tls.read(&mut asked).await.unwrap();
            tls.write_all(b"RTSP/1.0 200 OK\r\n\r\n").await.unwrap();
            tls.flush().await.unwrap();
            String::from_utf8_lossy(&asked[..n]).into_owned()
        });
        let mut roots = rustls::RootCertStore::empty();
        roots.add(cert).unwrap();
        let client = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let address = url::Url::parse(&format!("rtsps://localhost:{port}/cam/one?x=1")).unwrap();
        let (local, _task) = bridge(&address, std::sync::Arc::new(client)).await.unwrap();
        assert_eq!(local.scheme(), "rtsp");
        assert_eq!(local.host_str(), Some("127.0.0.1"));
        assert_eq!(local.path(), "/cam/one");
        assert_eq!(local.query(), Some("x=1"));
        let mut plain = tokio::net::TcpStream::connect(("127.0.0.1", local.port().unwrap()))
            .await
            .unwrap();
        plain
            .write_all(b"OPTIONS * RTSP/1.0\r\n\r\n")
            .await
            .unwrap();
        let mut answer = vec![0u8; 64];
        let n = plain.read(&mut answer).await.unwrap();
        assert!(answer[..n].starts_with(b"RTSP/1.0 200 OK"));
        assert!(served.await.unwrap().starts_with("OPTIONS * RTSP/1.0"));
    }

    #[test]
    fn a_sign_in_is_taken_out_of_the_address() {
        use crate::test_secrets::{escaped, marked, secret};
        // A password with the characters an address must escape.
        let (user, password) = (secret("rtsp_user"), marked("rtsp"));
        let address = format!("rtsp://{user}:{}@cam.example:554/s1", escaped(&password));
        let (address, creds) = split_credentials(&address).unwrap();
        assert_eq!(address.as_str(), "rtsp://cam.example:554/s1");
        let creds = creds.unwrap();
        assert_eq!((creds.username, creds.password), (user, password));
        let (_, none) = split_credentials("rtsp://cam.example/s1").unwrap();
        assert!(none.is_none());
    }
}
