# What dm does with your data

Short version: nothing leaves your machine. There is no account, no server of
ours, and no analytics.

## Looking for a new version

Only if you turn it on (Options › General, off by default), dm looks for a
newer version about once a week, as Help › Check for updates does when you
click it: it fetches the release's `latest.json` from GitHub. That request
carries nothing about you or your downloads; GitHub sees an address asking
for a public file, as it does for any download.

## Getting the tools for video sites

A video site's page (YouTube and the like) is downloaded with yt-dlp, which
needs a JavaScript runtime (Deno) for some sites; dm joins the picture and
sound itself. If your computer has no yt-dlp, dm fetches it and Deno the
first time such a page needs them, from their GitHub releases, each checked
against its published checksum before it is kept in dm's own folder. dm
does the rest itself: joining picture and sound, subtitles, and live RTMP,
RTSP and MMS streams. ffmpeg is fetched the same way (an ffmpeg build from
GitHub, checked against a checksum built into dm) only for what dm does not
do: the long-unused RTMPE and RTMPT, RTSP over TLS, and a stream whose
picture is MPEG-2. A yt-dlp
fetched this way is fetched afresh once a week while it is in use. These
requests, like any download, carry nothing about you; GitHub sees an address
asking for a public file. A yt-dlp or ffmpeg you installed yourself is used
instead, and then it is not fetched.

## What the extension handles

**Downloads your browser starts.** When the browser begins a download, the
extension looks at its address, the page it came from, its file name, type and
size, to decide whether the dm app on this computer should take it. If it
should, those — the address and the page — are passed to the app over the
browser's native messaging channel, which only reaches a program registered on
this computer. Nothing is sent anywhere else.

**Links and media you right-click.** Their address goes to the app the same
way, only when you ask. The toolbar popup's "Open dm" only brings the app's
window up: it sends no address at all. "Download all links" and "Download
selected links" read the addresses of the page's links, only when you pick
them; on a tab that was open before dm was installed, the extension first
puts its small links reader into that page (the `scripting` permission),
which reads link addresses and nothing else.

**Magnet links you click.** A click on a magnet link sends that one link to
the app, as a clicked `.torrent` file is; the page script reads nothing else
from the page. With catching turned off, in a private window, or with the
key that leaves a download to the browser held, the browser handles it as
it always would.

**The media a page's player loads.** To offer a video on sites no page
reader knows, the extension looks at the type and
size of each response a page receives (the `webRequest` permission, read
only, never changed). It keeps the address of audio, video and stream
playlists, for that tab only, in the browser's session memory, and forgets
them when the tab loads another page. Nothing is sent anywhere unless you
pick one in the panel.

**The video or audio you hover.** The extension puts a small
"Download video" panel over players. To do that it runs a page script
on every site, which is why the browser asks for access to all sites. The
script looks only for video and audio elements under the pointer. It reads
no text, forms or cookies and sends nothing until you click the panel. Then
it sends the player's address and the page's address to the extension's
background, which passes them to the app as above.

**Nothing else.** The only thing the extension stores is the app's catch
rules: the file types, size floor and skipped sites from dm's settings. They
are kept in the browser's extension storage on this computer, so a download
can be held at once rather than after the app has started. Whether the app
is there is kept in memory for a minute and then forgotten.

## What the app handles

The app downloads what it is given, with the page as referrer, and keeps its
queue and history on this computer.

**Your session on a video page.** When you ask dm for a video from a page —
the "Download video" panel, or "Download media with dm" — the extension
sends that page's cookies along, for that one video. Video sites turn away a
download that does not carry the session your browser has ("confirm you're
not a bot"). They go to the app on this computer only, are sent to that
page's server only (in a file of the download's own, readable by your
account alone and removed when the download ends), and are dropped from the
row once the video is downloaded. This is why the extension has the
browser's cookies permission.

**Sign-ins for other downloads.** Other downloads go without your cookies;
one that needs a sign-in is left to the browser — unless you switch "Hand
over this site's sign-in" on for that site in the extension's toolbar popup.
From then on that site's cookies travel with its downloads to the app, are
sent to that site's server only, and are kept with the row in its queue file
(readable by your account alone) — so that Redownload and the Recheck queue
can ask that server again — until you delete the download from dm's list.
Switching it off stops that for new downloads.

## What dm never does

- Send your downloads, addresses, browsing or settings to anyone.
- Include analytics, tracking, crash reporting or advertising.
- Read the pages you visit.

## Questions

dm is open source; if this document and the code ever disagree, the code is
the truth and the disagreement is a bug worth reporting.

This is also checked, not only promised: `cargo xtask privacy` (in
`desktop/`, and as a test in every build) fails if any crate that sends
reports — analytics, crash reporting, telemetry — enters the build, or if
shipped code names any address but GitHub's. dm reaches GitHub only when
you check for updates or open Help, or to fetch yt-dlp, Deno and ffmpeg
when a video needs them and your computer has none.
