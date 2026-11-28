# Speed limiter

The speed limiter keeps EDM from using all of your connection. This page
covers every place you set a limit: the **Speed limiter** window, the
**Downloads › Speed limiter** menu, the status bar, the tray icon's
**Speed** menu, **Options › Advanced**, a site rule's own limit, and one
download's own limit.

## The limit every download shares

There is one speed limit for all of EDM, in KB/s. It holds every connection
of every running download together under that speed; it is not a speed for
each download. Torrents are held under it too: while some run, the limit is
shared between them and EDM's other downloads. 0 means no limit.

You can set this one limit in four places, and each shows the same number:

- **Speed limit (KB/s, 0 for none)** in the Speed limiter window;
- **Options › Advanced › Speed limit (KB/s, 0 for none)** ("Keeps EDM from
  using all of your connection.");
- the presets in **Downloads › Speed limiter**, the status bar and the
  tray;
- **Limit speed**, which turns it off and on.

A change takes effect within a moment, on downloads already running.

## Downloads › Speed limiter

The menu, from top to bottom:

- **Limit speed:** ticked while a limit is in force. Click it to turn the
  limit off; click it again to turn it back on at the limit you had
  before. If no limit was ever set, it turns on at 512 KB/s.
- **Low speed (256 KB/s)**, **Medium speed (1 MB/s)** and **High speed (4
  MB/s):** set the limit to 256, 1024 or 4096 KB/s. The one in force is
  ticked.
- **Full speed:** no limit. Ticked while there is none.
- **Speed limiter settings…:** opens the Speed limiter window (below).

After a choice the status bar says "Speed limiter on: … KB/s." or "Speed
limiter off." A limit you turn off or replace with a preset is remembered,
so **Limit speed** can bring it back, also after EDM restarts.

## The status bar

The last words of the status bar, at the foot of the window, say "limited
to … KB/s" while a limit is in force, or "Speed limiter" while there is
none. Click them for the same choices as **Downloads › Speed limiter**.

## The tray icon's menu

The tray icon's menu has:

- **Speed limiter:** ticked while a limit is in force; click it to turn the
  limit off or back on, as **Limit speed** does.
- **Speed:** **Low speed (256 KB/s)**, **Medium speed (1 MB/s)**, **High
  speed (4 MB/s)** and **Full speed**, the one in force ticked, and
  **Speed limiter settings…**, which brings EDM's window forward and opens
  the Speed limiter window.

## The Speed limiter window

Open it with **Downloads › Speed limiter › Speed limiter settings…**, the
same item on the status bar, or **Speed › Speed limiter settings…** on the
tray icon. Asked for again while it is open, it comes to the front.

- **Speed limit (KB/s, 0 for none):** the limit every download shares, as
  above. The most it takes is 1,048,576 KB/s.
- **Data allowance (MB, 0 for none)** and **Allowance period (hours):** a
  download limit, for a connection with a
  fair-use allowance. EDM counts what every download brings in, torrents and
  videos included. The period begins with the first byte counted after the
  last period ended, and runs for the hours given (1 to 744). Once the
  allowance is used up, EDM holds its downloads until the period is over,
  then they go on by themselves. While it holds them the status bar says
  "download limit of … MB reached, held for …". What has been used is kept
  when EDM restarts.
- **Slower between set hours:** a lower limit for part of every day. Tick
  it, then choose **Start at** and **Stop at** (hours and minutes) and give
  **Speed limit in those hours (KB/s)**. A time span that ends before it
  starts runs past midnight. In those hours the lower of the two limits is
  in force; with no shared limit, the one for those hours is. The status
  bar and the tray's ticks show the shared limit, not this one.
- **Cancel:** closes the window and changes nothing.
- **OK:** puts the limits in force at once and closes the window.

A number that cannot be read keeps the value the field had before. Times
that cannot be read are refused with "… to …: times are HH:MM in 24 hours,
such as 01:00 or 23:30." and the window stays open.

**Return** in any field is **OK**; **Escape**, Ctrl+W (⌘W on macOS) and the
close button are **Cancel**.

## A site's own limit

**Options › Advanced › Sites with their own folder, speed and headers:**
holds rules for single sites. Each rule's **Speed limit, KB/s (0 for
none)** is given to a download from that site as its own limit (below)
when the download is added, unless that download was given a limit of its
own. A rule matches its site and the site's subdomains (a rule for
`example.com` also covers `files.example.com`); where two rules match, the
longer site name wins. Downloads added before the rule are not changed.

## One download's own limit

In a download's [progress window](download-window.md), tick **Limit this
download to** and type a speed in KB/s. That download is then held to its
own limit and to the shared one at the same time, so the lower of the two
is what it gets. Untick it to take the download's own limit away. A limit
from a site rule shows here too, ticked.

## Held downloads

When the download limit or the Scheduler holds downloads, **Start now** or
**Resume** on them asks **Start outside the schedule?** (see [Questions EDM
asks](questions.md)), and says which is holding them: "Held by the download
limit; it goes on by itself in …".
