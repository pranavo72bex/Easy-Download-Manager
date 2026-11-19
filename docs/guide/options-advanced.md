# Options › Advanced

The network, sites with rules of their own, the proxy, Dial Up / VPN and
BitTorrent. See [Options](options.md) for how the window works.

## Network

- **Speed limit, KB/s (0 for none):** "Keeps dm from using all of your
  connection." It is the same limit as **Downloads › Speed limiter**: see
  [Speed limiter](speed-limiter.md).
- **Attempts per connection (0 to keep trying):** "How many times dm tries
  again after a connection fails."
- **Wait between attempts (seconds, 0 for automatic):** "How long dm waits
  before it tries again." 0 lets dm choose: quick at first, then half a
  minute apart.
- **Time out a silent connection after (seconds):** "How long a connection
  may say nothing before dm gives up on it." It is then tried again, as
  above.
- **Only through:** "Every download goes out over this network alone, such
  as your VPN's. While it is not connected, downloads wait and say so, and go
  on by themselves when it is back. Torrents and live streams do not start
  while one is chosen, and the second network is not used." The pop-up lists
  the computer's networks, as **Second network** does; **Any network**, the
  first, lets downloads go out however the computer sends them. With one
  chosen, every connection a download makes (and dm's own fetches, such as a
  video page's) goes out from that network's address, so when a VPN drops
  nothing goes over the plain line: running downloads stop and wait, their
  rows saying **Waiting for the network**, and go on by themselves within a
  few seconds of it coming back. A torrent or a live stream (RTMP, RTSP, MMS)
  fails saying why instead, since dm cannot hold its connections to one
  network. While one is chosen, **Second network** is greyed and not used.
- **Second network:** "Use a second connection too, such as your phone over
  USB or its hotspot. Half of each download's connections go over it. Not
  used while a proxy is on." **None** uses only the computer's usual
  connection.
- **How to use your phone** shows the steps:
  - "1. Connect your phone to this computer with its USB cable. If it asks
    whether to trust this computer, say yes."
  - "2. Turn on its hotspot: on an iPhone, Settings › Personal Hotspot; on
    Android, USB tethering."
  - "3. Choose it above. Keep Wi-Fi as this computer's own connection, so
    only downloads use the phone's data."

## Sites with rules of their own

- **Sites with their own connection limit:** for a site that refuses or
  slows down many connections at once. **Add a site limit** adds a line with
  **Site** and **Connections**; **Delete site limit** takes it away.
- **Sites with their own folder, speed and headers:** **Add a site rule**
  adds a line for a **Site** (it covers its subdomains), with:
  - **Folder:** where its downloads are saved, in place of the folder their
    kind has; empty keeps that. **Browse…** picks one.
  - **Speed limit, KB/s (0 for none):** a limit for each of its downloads.
  - **User-Agent:** the name its downloads are asked with, in place of the
    one dm picks; empty keeps dm's.
  - **Request headers:** headers its downloads carry, written `Name: value`,
    each apart by `|`.
  - **Delete site rule** takes the line away.
- "For downloads that need a sign-in: a cookies.txt from your browser."
  **Cookies file:** a `cookies.txt` file saved from your browser; **Choose…**
  picks it and **Clear** stops using it. See [Sign-ins](sign-ins.md).
- "dm identifies itself to servers as:" shows the name dm sends to servers.
- **Identify as dm only:** "Sends dm's own name first. A site that refuses
  it is asked again with another name." Off, dm first sends a name like a
  browser's.

## Proxy

**Connect through a proxy**, **Proxy type**, **Host**, **Port**, **Username,
if it needs one**, **Password**, **Not for these sites (fetched directly)**,
**A proxy of its own for HTTP downloads**, **A proxy of its own for HTTPS
downloads** (each with **Its host** and **Its port**), and the **Automatic
configuration script address (it chooses each site's proxy, in place of the
above)**: see [Proxies](proxies.md).

## Dial Up / VPN (Windows)

For a connection Windows dials, such as a VPN, that downloads need.

- **Use Windows Dial Up / VPN Networking:** turns it on.
- **Connection:** one of the dial-up and VPN connections Windows knows, or
  **None**.
- **User name** and **Password:** for that connection.
- **Save password:** keeps the password after dm closes.
- **Redial attempts (zero if endlessly)** and **Time between redial attempts
  (seconds)**: how often dm tries to bring the connection up.
- "A connection Windows knows (Settings › Network & internet › Dial-up or
  VPN). A download waits while dm connects it, and stops if it does not come
  up. With no user name or password here, dm uses the ones Windows keeps;
  without Save password, the one typed is forgotten when dm closes."

The [Scheduler](scheduler.md) can hang this connection up when the scheduled
downloads are done.

## BitTorrent

- **Block the peers listed at this address (optional):** "A list in the P2P
  format, plain or gzipped, on the web or on this computer. One on the web
  is fetched through the proxy and read again once a day; a torrent stops,
  saying why, if the list cannot be read."
- **Keep sharing a finished torrent:** "Its files stay where they are, and
  other peers are sent them until it has sent the times its size below, or
  for the minutes below, whichever comes first. Off, a torrent stops once
  its files are whole." It is off unless you turn it on, since sharing sends
  from your connection.
  - **Until it has sent (times its size)**
  - **For at most (minutes)**
  - **Send at most (KB/s, 0 for no limit)**
- "The first torrent makes Windows ask whether dm may use public and private
  networks. Allowed, other peers can reach dm and torrents go faster;
  refused, dm still downloads, reaching peers itself. Change it later in
  Windows Security, Firewall, Allow an app through firewall."
