//! One logged-in control connection and the commands run on it.

use super::*;

/// One signed-in control connection.
pub(super) struct Session {
    pub(super) reader: BufReader<ReadHalf<Conn>>,
    pub(super) writer: WriteHalf<Conn>,
    /// Where the data connection goes: the address the control connection
    /// reached, or, through a proxy (whose address is all this end sees),
    /// the server's name for the proxy to resolve.
    pub(super) data_host: String,
    pub(super) route: Option<Route>,
    /// The TLS the data connections are spoken in, and the server's name,
    /// for an `ftps://` address: one config, so each data connection can
    /// take up the control connection's TLS session, as servers ask.
    pub(super) tls: Option<(tokio_rustls::TlsConnector, ServerName<'static>)>,
}

impl Session {
    pub(super) async fn open(target: &Target) -> Result<Session, Error> {
        let tls = match target.tls {
            None => None,
            Some(_) => {
                let config = match &target.trust.0 {
                    Some(config) => Arc::clone(config),
                    None => crate::rtmp::tls_config()?,
                };
                let name = ServerName::try_from(target.host.clone()).map_err(|e| {
                    Error::Internal(format!("{} is not a name TLS can check: {e}", target.host))
                })?;
                Some((tokio_rustls::TlsConnector::from(config), name))
            }
        };
        let stream = connect(target.route.as_ref(), &target.host, target.port).await?;
        // The data connection goes to the address reached, not to the name
        // looked up again: a server answering its own DNS could otherwise
        // point the second lookup at this computer or its network.
        let data_host = if target.route.is_some() {
            target.host.clone()
        } else {
            stream.peer_addr().map_err(dropped)?.ip().to_string()
        };
        let stream: Conn = match (&tls, target.tls) {
            (Some((connector, name)), Some(Tls::Implicit)) => {
                Box::new(handshake(connector, name, Box::new(stream)).await?)
            }
            _ => Box::new(stream),
        };
        let (read, writer) = tokio::io::split(stream);
        let mut session = Session {
            reader: BufReader::new(read),
            writer,
            data_host,
            route: target.route.clone(),
            tls: tls.clone(),
        };
        let (code, text) = session.reply().await?;
        if code != 220 {
            return Err(refusal(code, &text, &target.path));
        }
        // Explicit TLS: asked for, and the same connection carried on in it.
        if let (Some((connector, name)), Some(Tls::Explicit)) = (&tls, target.tls) {
            let (code, text) = session.command("AUTH TLS").await?;
            if code != 234 {
                return Err(Error::Internal(format!(
                    "the FTP server would not speak TLS ({})",
                    text.trim()
                )));
            }
            let Session {
                reader,
                writer,
                data_host,
                route,
                tls,
            } = session;
            let plain = reader.into_inner().unsplit(writer);
            let secure: Conn = Box::new(handshake(connector, name, plain).await?);
            let (read, writer) = tokio::io::split(secure);
            session = Session {
                reader: BufReader::new(read),
                writer,
                data_host,
                route,
                tls,
            };
        }
        let (code, text) = session.command(&format!("USER {}", target.user)).await?;
        let (code, text) = if code == 331 {
            session
                .command(&format!("PASS {}", target.password))
                .await?
        } else {
            (code, text)
        };
        if code != 230 && code != 202 {
            return Err(refusal(code, &text, &target.path));
        }
        // The data connections in TLS too, never in the clear.
        if session.tls.is_some() {
            for command in ["PBSZ 0", "PROT P"] {
                let (code, text) = session.command(command).await?;
                if code != 200 {
                    return Err(Error::Internal(format!(
                        "the FTP server would not keep the data in TLS ({})",
                        text.trim()
                    )));
                }
            }
        }
        let (code, text) = session.command("TYPE I").await?;
        if code != 200 {
            return Err(refusal(code, &text, &target.path));
        }
        Ok(session)
    }

    /// Reads one reply, however many lines it runs to.
    ///
    /// Bounded twice over: in bytes, read no further than the cap allows
    /// (a line is not read whole and then measured, or a server sending one
    /// endless line fills memory first), and in time, the whole reply
    /// within one timeout (a line every 29 seconds would otherwise keep a
    /// download for days).
    pub(super) async fn reply(&mut self) -> Result<(u16, String), Error> {
        tokio::time::timeout(TIMEOUT, self.reply_unbounded_in_time())
            .await
            .map_err(|_| dropped("the FTP server stopped answering"))?
    }

    pub(super) async fn reply_unbounded_in_time(&mut self) -> Result<(u16, String), Error> {
        let mut used = 0usize;
        let mut first: Option<u16> = None;
        loop {
            let mut bytes = Vec::new();
            let left = (MAX_REPLY - used.min(MAX_REPLY)) as u64 + 1;
            let read = (&mut self.reader)
                .take(left)
                .read_until(b'\n', &mut bytes)
                .await
                .map_err(dropped)?;
            if read == 0 {
                return Err(dropped("the FTP server closed the connection"));
            }
            used += read;
            if used > MAX_REPLY || bytes.last() != Some(&b'\n') {
                return Err(Error::Internal("the FTP server's reply ran on".into()));
            }
            let line = String::from_utf8_lossy(&bytes);
            let line = line.trim_end();
            match (first, reply_code(line)) {
                (None, Some((code, true))) => return Ok((code, line.to_string())),
                (None, Some((code, false))) => first = Some(code),
                (None, None) => {
                    return Err(Error::Internal(format!(
                        "the FTP server answered something that is not FTP: {}",
                        line.chars().take(80).collect::<String>()
                    )))
                }
                (Some(code), Some((same, true))) if same == code => {
                    return Ok((code, line.to_string()))
                }
                // A line inside a multi-line reply.
                (Some(_), _) => {}
            }
        }
    }

    pub(super) async fn command(&mut self, command: &str) -> Result<(u16, String), Error> {
        timed(self.writer.write_all(format!("{command}\r\n").as_bytes())).await?;
        self.reply().await
    }

    /// Opens a passive data connection: `EPSV`, or `PASV` where that is
    /// refused. Plain still: [`Session::secure`] starts its TLS once the
    /// command it carries has been answered.
    pub(super) async fn data(&mut self, path: &str) -> Result<TcpStream, Error> {
        let (code, text) = self.command("EPSV").await?;
        let port = if code == 229 {
            epsv_port(&text)
        } else {
            let (code, text) = self.command("PASV").await?;
            if code != 227 {
                return Err(refusal(code, &text, path));
            }
            pasv_port(&text)
        };
        let port = port.ok_or_else(|| {
            Error::Internal("the FTP server's passive reply named no port".into())
        })?;
        connect(self.route.as_ref(), &self.data_host, port).await
    }

    /// A data connection as the transfer is spoken: in TLS for `ftps://`,
    /// begun now, after the server has answered the command it carries.
    pub(super) async fn secure(&self, data: TcpStream) -> Result<Conn, Error> {
        match &self.tls {
            Some((connector, name)) => Ok(Box::new(Lenient(Box::new(
                handshake(connector, name, Box::new(data)).await?,
            )))),
            None => Ok(Box::new(data)),
        }
    }

    /// Starts sending `path` from `offset`.
    pub(super) async fn retrieve(&mut self, path: &str, offset: u64) -> Result<Conn, Error> {
        let data = self.data(path).await?;
        if offset > 0 {
            let (code, text) = self.command(&format!("REST {offset}")).await?;
            if code != 350 {
                return Err(refusal(code, &text, path));
            }
        }
        let (code, text) = self.command(&format!("RETR {path}")).await?;
        if code != 150 && code != 125 {
            return Err(refusal(code, &text, path));
        }
        self.secure(data).await
    }
}

/// A TLS handshake as the client, within the timeout.
pub(super) async fn handshake(
    connector: &tokio_rustls::TlsConnector,
    name: &ServerName<'static>,
    stream: Conn,
) -> Result<tokio_rustls::client::TlsStream<Conn>, Error> {
    timed(connector.connect(name.clone(), stream))
        .await
        .map_err(|e| Error::Internal(format!("the FTP server's TLS failed: {e}")))
}
