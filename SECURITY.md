# Security

## Reporting a vulnerability

Please do not open a public issue for a security problem. Report it
privately through GitHub: the repository's **Security** tab, then **Report a
vulnerability**. Only the maintainer sees it.

Say what an attacker can do, and how: a link, a page, a server's reply, a
file, or a message to the native host or the control port that makes dm do
something it should not. A proof of concept helps but is not required.

You will get an answer within a week. Once a fix is released, the advisory
is published with credit to you, unless you would rather not be named.

## What is supported

Only the latest release. dm has no long-term branches; a fix ships as a new
release, and dm says when one is out (About › Check for updates).

## What counts

dm reads a great deal of input it does not control, so these matter most:

- a server's reply, a playlist, a Metalink, a `.torrent` or a ZIP that writes
  outside the download folder (or outside the folder dm extracts it into),
  or crashes or hangs dm;
- a web page that gets the extension to hand dm something the person did
  not ask for, or to send the browser's cookies where they were not allowed;
- anything that reaches the native host or the control port (`--control`)
  without its token;
- settings, the queue or sign-ins readable by other users of the computer.

The builds are not code-signed; an operating system's warning about that is
expected, not a vulnerability.

## Known advisories in dependencies

`cargo audit` runs on every change and weekly (`.github/workflows/audit.yml`).
Advisories that were looked at and are unreachable in dm are listed in
`desktop/.cargo/audit.toml`, each with the reason.
