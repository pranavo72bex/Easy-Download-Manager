# Proxies

A proxy is a server your downloads go through, for a network that reaches
the internet only that way (an office's, a school's). Whoever runs the
network can give you its address. The settings are in **Options › Advanced
› Proxy**.

- **Connect through a proxy:** "For a network that reaches the internet
  through a proxy server. Whoever runs the network can give you its
  address." Off, downloads go out directly.
- **Proxy type:**
  - **The computer’s proxy settings:** the proxy Windows (or your system) is
    set to use. The fields below are greyed.
  - **HTTP**, **HTTPS**, **SOCKS4** or **SOCKS5:** a proxy you name below.
    Ask whoever runs the network which kind it is. Changing the kind changes
    the port to that kind's usual one (8080 for HTTP and HTTPS, 1080 for
    SOCKS) unless you had typed your own.
- **Host:** the proxy's name or address alone, such as `proxy.example.com`;
  no `http://`, no path.
- **Port:** its port, from 1 to 65535.
- **User name, if it needs one** and **Password:** the proxy's own login.
  The password is kept in the system's password store where it works.
- **Not for these sites (fetched directly):** sites that do not go through
  the proxy, separated by spaces or commas: a host, a domain (covering its
  subdomains), an address, or `*` for every site.
- **A proxy of its own for HTTP downloads** and **A proxy of its own for
  HTTPS downloads:** tick one to send that kind of download through another
  proxy, given by **Its host** and **Its port**; the rest go through the
  one above.
- **Automatic configuration script address (it chooses each site's proxy,
  in place of the above):** the address of a proxy configuration script (a
  web address, a `file:` address or a path), when the network gives you one
  rather than a proxy. The sites on the bypass list still go directly.
- "Every download, the video tools and a torrent's SOCKS5 connections go
  through it; FTP through SOCKS or HTTP CONNECT."

## What goes through it

- Web downloads (HTTP and HTTPS), video pages and their streams.
- FTP, through a SOCKS or an HTTP proxy. EDM cannot send FTP through an HTTPS
  proxy and does not go around it: use an HTTP or SOCKS proxy, or put the
  site on the bypass list.
- Torrents only through a SOCKS5 proxy. With a proxy on, a torrent finds
  peers only through its `http` and `https` trackers. A peer blocklist on
  the web is fetched through the proxy too.
- Live streams (`rtmp://`, `rtsp://`, `mms://`) cannot go through a proxy,
  and EDM does not send them around it: turn the proxy off for that site
  (put it on the bypass list) to record one.
- **Second network** (Options › Advanced › Network) is not used while a
  proxy is on: the proxy is the way out.

## When OK refuses

Options will not keep a proxy that is turned on but cannot be used, and
opens Advanced at its proxy section to say why:

- "Enter the proxy’s host, or untick “Connect through a proxy”."
- "… isn't a valid proxy host. Enter a name like proxy.example.com; the
  port goes in its own field."
- "A proxy needs a port from 1 to 65535."

A download that fails at the proxy says so: see the proxy section of
[What EDM's messages mean](failures.md).
