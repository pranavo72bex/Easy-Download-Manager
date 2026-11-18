# Sign-ins

Some files are only given to someone signed in. dm can sign in in several
ways; use the one that fits the site.

## The browser's sign-in

For a site you are signed in to in your browser, the easiest way is to let
the browser hand the download over: click it on the page, or right-click
it and choose **Download link with dm**.

- **Options › Browser › Send the browser's sign-in with downloads:** "Files
  that need you signed in come to dm too. Your sign-in goes with that one
  download and is forgotten after it. Off: only the sites you turn on in the
  extension's menu send it."
- A link that works only while you are signed in, or only for a while,
  stops working later. dm then asks for a fresh one: see [Update download
  link](update-link.md) and **Offer a new link when a download's link has
  expired** in [Options › General](options-general.md).

## A login for a site

**Options › Sign-ins** keeps a user and password for a host, used for every
download from it: see [Options › Sign-ins](options-sign-ins.md). It is sent
only over `https://`. dm signs in with whatever the server asks for: Basic,
Digest, or NTLM (Windows' own sign-in, which some company servers use).

You can also put the login in the address itself: after `https://`,
write the name, a colon, the password and `@`, then the server's name.
A login in the address is used in place of the one Options keeps.

## A cookies file

Some sites sign you in with cookies rather than a login. Save the site's
cookies from your browser as a `cookies.txt` file (a browser add-on can
write one) and choose it in **Options › Advanced › Cookies file**. dm sends
the cookies in it with downloads from those sites. **Clear** stops using it.

## FTP

An FTP server's login goes in the address: `ftp://` followed by
`name:password@` and the server. Without one, dm signs in as a guest
(anonymous), which many FTP servers refuse.

## The proxy's login

A proxy that needs a login has its own **Username, if it needs one** and
**Password**: see [Proxies](proxies.md).

## When the sign-in fails

The server answers 401 when it wants a sign-in dm did not have or that was
wrong, and 403 when it refuses you (often a link that has run out). See
[What dm's messages mean](failures.md).
