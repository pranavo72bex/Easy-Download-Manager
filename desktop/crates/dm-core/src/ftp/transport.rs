//! The connection types: plain or TLS, and the lenient close FTP servers need.

use super::*;

/// A connection, plain or TLS.
pub(super) trait Io: AsyncRead + AsyncWrite + Unpin + Send + Sync {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + Sync> Io for T {}

pub(super) type Conn = Box<dyn Io>;

/// How an `ftps://` address is spoken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Tls {
    /// TLS from the first byte, on port 990.
    Implicit,
    /// Asked for with `AUTH TLS` before signing in.
    Explicit,
}

/// The TLS settings a session is spoken with: the system's roots, or in a
/// test, a certificate of the test's own. Never compared: two targets are
/// the same address whatever trusts them.
#[derive(Clone, Default)]
pub(super) struct Trust(pub(super) Option<Arc<ClientConfig>>);

impl std::fmt::Debug for Trust {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Trust")
    }
}

impl PartialEq for Trust {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Trust {}

/// A TLS data connection that a server closes without saying so first,
/// as many do: its end, not a failure. Whether the file is whole is
/// judged by its size and the server's own reply, as over plain FTP.
pub(super) struct Lenient(pub(super) Conn);

impl AsyncRead for Lenient {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match std::pin::Pin::new(&mut self.0).poll_read(cx, buf) {
            std::task::Poll::Ready(Err(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                std::task::Poll::Ready(Ok(()))
            }
            other => other,
        }
    }
}

impl AsyncWrite for Lenient {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.0).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.0).poll_shutdown(cx)
    }
}

/// How long a server may stay silent before the attempt is given up and
/// retried, as the HTTP client's read timeout does.
pub(super) const TIMEOUT: Duration = Duration::from_secs(30);

/// The longest reply dm reads. A server that sends more is not answering.
pub(super) const MAX_REPLY: usize = 64 * 1024;
