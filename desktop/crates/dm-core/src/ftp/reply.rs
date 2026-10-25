//! Reading the server's replies, and the errors they become.

use super::*;

/// A reply's code, and whether this line ends it (`"230 "`) or more lines
/// follow (`"230-"`). `None` for a line that is neither.
pub(super) fn reply_code(line: &str) -> Option<(u16, bool)> {
    let code = line.get(..3)?.parse::<u16>().ok()?;
    if !(100..600).contains(&code) {
        return None;
    }
    match line.as_bytes().get(3) {
        Some(b' ') | None => Some((code, true)),
        Some(b'-') => Some((code, false)),
        _ => None,
    }
}

/// The port in an `EPSV` reply: `229 Entering Extended Passive Mode (|||6446|)`.
pub(super) fn epsv_port(reply: &str) -> Option<u16> {
    let open = reply.find('(')?;
    let inner = reply.get(open + 1..)?;
    let close = inner.find(')')?;
    let inner = inner.get(..close)?;
    let delimiter = inner.chars().next()?;
    let mut fields = inner.split(delimiter);
    // "", "", "", port, ""
    fields.nth(3)?.parse().ok()
}

/// The port in a `PASV` reply: `227 Entering Passive Mode (h1,h2,h3,h4,p1,p2)`.
/// The address in it is not used: the data connection goes to the server
/// the control connection reached, as curl does, which is right behind a
/// NAT that reports its inside address and never somewhere else entirely.
pub(super) fn pasv_port(reply: &str) -> Option<u16> {
    // After the parenthesis where there is one, else after the code.
    let from = reply.find('(').map_or(4, |p| p + 1);
    let numbers: Vec<u16> = reply
        .get(from..)?
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .take(6)
        .map(|s| s.parse::<u16>().ok())
        .collect::<Option<_>>()?;
    let (&high, &low) = (numbers.get(4)?, numbers.get(5)?);
    if high > 255 || low > 255 {
        return None;
    }
    Some(high * 256 + low)
}

/// A number after the code: `213 1048576`.
pub(super) fn number_in(reply: &str) -> Option<u64> {
    reply.get(4..)?.trim().parse().ok()
}

/// A dropped connection, a timeout or a busy server: worth another try.
pub(super) fn dropped(e: impl std::fmt::Display) -> Error {
    Error::Connection(e.to_string())
}

pub(super) async fn timed<T>(
    work: impl std::future::Future<Output = std::io::Result<T>>,
) -> Result<T, Error> {
    tokio::time::timeout(TIMEOUT, work)
        .await
        .map_err(|_| dropped("the FTP server stopped answering"))?
        .map_err(dropped)
}

/// Runs FTP's back and forth, stopping as soon as a pause or a cancel is
/// asked for rather than when the server next answers.
pub(super) async fn or_stop<T>(
    control: &crate::control::Control,
    work: impl std::future::Future<Output = Result<T, Error>>,
) -> Result<T, Error> {
    tokio::pin!(work);
    loop {
        tokio::select! {
            done = &mut work => return done,
            _ = tokio::time::sleep(Duration::from_millis(250)) => control.check()?,
        }
    }
}

/// The error a reply that is not the one hoped for comes to.
pub(super) fn refusal(code: u16, text: &str, path: &str) -> Error {
    let said = text.get(4..).unwrap_or("").trim();
    match code {
        // "Try again later": too many users, a data connection that failed.
        400..=499 => Error::Connection(format!("the FTP server said {code} {said}")),
        530 => Error::Internal(format!(
            "the FTP server refused the sign-in ({said}) — put the name and password in the address: ftp:// followed by name:password@ and the server"
        )),
        550 => Error::Internal(format!("the FTP server has no file {path} ({said})")),
        _ => Error::Internal(format!("the FTP server answered {code} {said}")),
    }
}
