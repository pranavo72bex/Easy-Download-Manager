# What dm's messages mean

This page lists every sentence dm can put on a download's row when the
download fails, and every note a row can carry, with what each means and
what you can do next.

## Where a failure shows

- **The Status column** says **Error:** and the reason. The common reasons
  are put in plainer words there ("this link no longer works"). Hold the
  mouse over the Status to read the whole of it, with dm's own sentence
  under the plainer words. dm's own sentence is the one this page lists.
- **Properties** (on the row's right-click menu) shows the same failure,
  with dm's own sentence.
- **The download's window** shows the Status, a next step where dm knows
  one, and a button for it beside **Retry**: **Choose another folder…**,
  **Update download link**, **Open the page**, **Move/Rename** or
  **Options**.
- **Copy details**, on a failed row's right-click menu, copies a short block
  to paste to whoever is helping you: dm's version, the state and the
  reason, the server's name, the status code, the names of the headers
  sent, the number of connections and the free disk space. It holds no
  cookie, login, path or full address.
- Extracting an archive says **Not extracted:** and the reason in the
  status bar at the foot of the window.

**dm tries again by itself.** When a connection drops, times out or the
server says "later", dm waits and tries again, and goes on from what it
already has. The Status says so while it waits, for example "Connection
dropped (retry 2 of 15) · next try in 15s" or "Retrying (attempt 3)".
**Options › Advanced › Network** sets how often: **Attempts per connection
(0 to keep trying)** and **Wait between attempts (seconds, 0 for
automatic)**. A row shows **Error:** only once those attempts have run out,
or when the failure is an answer that trying again would not change.

**To try again yourself**, right-click the row and choose **Retry**, or
click **Retry** in the download's window. The toolbar's resume button reads
**Retry** too when the rows you chose have failed. A download that was
stopped, not failed, goes on with **Resume download** on the right-click
menu, or **Resume** in its window. Most downloads go on from the bytes they
already have.

In the sentences below, **…** stands for what the window fills in: a name,
an address, a number, or the words of the server or of Windows.

## Notes beside a download's state

A note follows the state after a dot ("Complete · …"). A note is not a
failure: the download worked.

- `higher qualities are copy-protected, so the best one that is not was taken`
  The stream offered better qualities, but those are copy-protected (DRM),
  which only the site's own player can play. dm took the best quality that
  is not protected. Nothing to do.
- `… pieces of the live stream were gone before dm could fetch them`
  While dm recorded a live stream, the stream moved on past that many
  pieces before dm fetched them, so the recording has a gap there. The rest
  of the recording is kept.
- `checked against the checksum file published beside it`
  The site published a checksum file next to the file (for example
  `name.sha256`). dm found it and the finished file matched it, so the file
  is the one the site published.
- `… parts of the stream in another format, such as an advert, were left out`
  The stream is made of several parts, and that many were unlike the rest
  (often an advert). dm left them out so the file is one video. Nothing to
  do.

A YouTube premiere, or a live stream that has not begun, is waited for: its
row says "Waiting:" and one of these until it begins, then records it as a
live stream. **Pause** or **Cancel** stops the wait; started again, the
download reads the video afresh and waits again.

- `the premiere starts in … hours`
- `the premiere starts in … minutes`
- `the premiere starts in a moment`
  How long is left until the time YouTube gives for it. dm reads the video
  again a few seconds after that time. Nothing to do: leave dm open.
- `the premiere has not begun yet`
  Its time has passed, but the stream has not come up. dm reads the video
  again every half minute, for up to six hours past its time.

**Unpack when done** ([Options › Folders](options-folders.md#unpack-when-done),
or one download's box in [Download details](download-details.md)) says what
it made of a finished archive:

- `unpacked into a folder of its name`
  Its files are in a new folder beside it, named after it (numbered, such as
  "pack (1)", where that name was taken). The archive is kept.
- `unpacked into a folder of its name, and the archive sent to the Trash`
  As above, and the archive is in the Recycle Bin (the Trash on macOS), as
  **Send the archive to the Trash** asks. The row stays, saying this rather
  than "File missing".
- `unpacked into a folder of its name, but the archive could not be sent to the Trash`
  Its files are out, but the system would not take the archive (on a drive
  with no Recycle Bin, for one). It is still where it was: delete it
  yourself if you no longer want it.
- `not unpacked: the archive has a password`
  The archive is locked with a password, which dm does not ask for yet.
  Nothing was written and the archive is kept. Unpack it with a program
  that asks for the password.
- `not unpacked: Extract here on its menu says why`
  dm refused to unpack it: a file in it would land outside its folder, it
  holds a link, it is damaged, too large, or the disk refused. Nothing was
  written and the archive is kept. **Extract here** on the row's
  right-click menu tries again and says the reason (see
  [Archives (extracting)](#archives-extracting)).

## Stopped, paused and cancelled

- `paused`
  The download was stopped on request: **Stop download**, the Scheduler's
  hours, or dm being closed. It is not a failure. What came is kept, and
  **Resume download** goes on from it.
- `cancelled`
  The download was cancelled. It is not tried again.
- "Waiting: the connection is metered"
  Options › Downloads › On a metered connection says to wait (or to ask,
  and **Wait** was chosen, or no window is open to ask in), and the system
  marks the connection as metered. It is not a failure: the download goes
  on by itself once the connection is not metered. See
  [Options › Downloads](options-downloads.md).
- "Waiting: the battery is low"
  The computer runs on its battery below the level set in Options ›
  Downloads › On battery: wait below. The download goes on by itself once
  the computer charges or the battery is above that level again.

## The server's answer

- `the server answered … for …`
  The server refused. The status code and the address follow, and, after a
  dash, the server's own words where they say something. What the common
  codes mean:
  - **401 Unauthorized:** the server wants a sign-in. Add the name and
    password in **Properties › Login**, or in **Options › Sign-ins** for the
    whole site, then click **Retry**.
  - **403 Forbidden** and **410 Gone:** the server will not give out the
    file at that address. A link from your browser is often signed and
    stops working after a while. If the download came from the browser, dm
    keeps what it has and the row waits for a new link (see "Web pages
    instead of files, and links that expired" below). A 403 can also mean
    the site wants you to sign in (**Properties › Login**).
  - **404 Not Found:** there is no file at that address now. The Status
    says "this link no longer works". Open the page the file came from in
    your browser and download it from there, or use **Update download link**
    with the new address.
  - **408, 425, 429, and every 5xx except 501:** the server is busy or
    having trouble. dm treats these as "try later" and tries again by
    itself; the Status says "Too many requests" or "Server error" with the
    attempt. Where the server says how long to wait, dm waits at least that
    long. If the attempts run out, try again in a few minutes.
  - **Any other code** (501, and the other 4xx) is an answer that trying
    again at once would not change.
- `the server sent dm on to …, an address dm does not follow`
  The server redirected to an address of a kind dm does not download from
  this way (for example a `file://` address). Open the link in your browser
  to see where it goes.
- `too many redirects: the address was sent on more than … times, last to …`
  The site sent dm from address to address more than ten times, which is
  a loop. Click **Open the page** in the download's window, or open the
  page in your browser, and download from there: that hands dm the real
  link.
- `the address sent dm on to one inside this computer or its own network, which a public site has no business reaching, so dm stopped`
  A site out on the internet sent dm to this computer or to your own
  network: by a redirect, by an address in a playlist or mirror list, or
  by a name that leads there. dm does not let a public site reach what it
  cannot reach itself. If the address really is yours (a server at home
  or at work), give dm that address directly, in **Add URL**.
- `the server sent … bytes of a file it said was …`
  The server ended the file before the size it had announced. Click
  **Retry**; dm asks again for the bytes that are missing.
- `the file changed on the server, so the download has to start again`
  The site put up a new version of the file while dm was fetching it, so
  the parts already fetched belong to a version that is gone. dm starts
  again once by itself; you see this when the file changed again. **Retry**
  fetches the version that is there now, from the beginning.
- `this server does not serve ranges honestly`
  Asked for one part of the file, the server sent bytes that are not that
  part (usually a compressed copy). Where dm used several connections, it
  has already fetched the file again over one. Click **Retry**; if it keeps
  happening, the fault is the server's.
- `the server stopped serving parts of the file`
  Asked for a part of the file, the server sent the whole file instead.
  dm fetches it again on one connection from the beginning by itself; you
  see this when that happened again. Try again later.

## The connection

- `the connection ended after … of … bytes`
  The connection dropped before its part was complete. dm tries again by
  itself, from where it stopped; you see this when the attempts ran out.
  Check that you are online, then click **Retry**.
- `the server's name could not be found (check the address)`
  The server's name did not lead to any server. Check the address for a
  typing mistake, and that you are online. Windows' own words follow.
- `the server refused the connection`
  Nothing at that address took the connection. Check the address, or try
  again later. If a firewall or security program asks about dm, allow it,
  then click **Retry**.
- `the server did not answer in time`
  The server said nothing for too long. dm tries again by itself; if the
  attempts run out, try again in a moment. **Options › Advanced › Network**
  sets how long a silent connection may last.
- `network: …`
  Another network failure, in the network library's own words, with each
  cause after it. Where the words name a certificate, the site's security
  certificate could not be checked: check that the computer's date and time
  are right, then click **Retry**. Otherwise check that you are online and
  click **Retry**. This prefix also comes before an FTP server's "try
  later" answers and dropped FTP connections, which dm tries again by
  itself.
- `TLS: …`
  dm could not set up secure connections at all. This should not happen.
  Close and open dm again; if it stays, use **Copy details** and report it.
- `the connection to dial could not be waited for: …`
  dm waits for the Windows dial-up or VPN connection set in **Options ›
  Advanced** before a download, and the waiting itself failed. This should
  not happen. Click **Retry**.
- `the network … that downloads go only through is not connected`
  Shown on a waiting row as **Waiting for the network**: downloads are held
  to one network (**Options › Advanced › Network › Only through**), such as
  a VPN's, and it has no address now. Nothing is sent meanwhile. dm looks for
  it every few seconds and goes on by itself once it is back; to download
  without it, choose **Any network** there.
- `… cannot be held to one network, and downloads go only through …, so dm did not start it: choose Any network under Options › Advanced › Network to fetch it`
  A torrent, or a live stream (RTMP, RTSP, MMS), makes connections dm cannot
  hold to the one network chosen in **Options › Advanced › Network › Only
  through**, so it is not started rather than sent around it. Choose **Any
  network** there, then click **Retry**.
- `… has no address the network chosen can reach`
  An FTP server (or the proxy in front of it) has only addresses of a kind
  the network chosen in **Only through** has none of, such as an IPv6
  server on a VPN with an IPv4 address alone. Choose **Any network**, or
  another network, then click **Retry**.
- `the NTLM sign-in could not be made: no random numbers to make it with`
  The server asked for a Windows network sign-in (NTLM), which needs random
  numbers from Windows, and none came. This should not happen. Click
  **Retry**, or close and open dm again.

## Proxies

The proxy is set in **Options › Advanced**, under **Proxy**. Sites listed in
**Not for these sites (fetched directly)** do not go through it.

- `the proxy configuration script gave no way to this site: …`
  The automatic configuration script chooses each site's proxy, and for
  this site it gave none that dm can use, or could not be read. The reason
  follows. Check the script's address, or ask whoever runs the network.
- `the proxy address is not usable: …`
- `the proxy address names no host`
- `the proxy address names no port`
  The proxy's address could not be read for an FTP download. Check its host
  and port in **Options › Advanced**.
- `dm cannot send FTP through an HTTPS proxy, and does not go around it — use an HTTP or SOCKS proxy, or put this site on the proxy's bypass list`
- `dm cannot send FTP through a … proxy, and does not go around it`
- `dm cannot send FTP through this kind of proxy`
  FTP goes through an HTTP or SOCKS proxy only, and dm never goes around a
  proxy you set. Choose an HTTP or SOCKS proxy, or add the FTP server to
  **Not for these sites (fetched directly)**. "The computer's proxy
  settings" cannot carry FTP either.
- `the proxy's answer ran on`
  The proxy's answer to an FTP connection did not end where an answer
  should. Check that the address is a proxy's.
- `a …:// stream cannot go through a proxy, and dm does not send it around one: turn the proxy off for this site (Options › Advanced) to fetch it`
  A live stream (RTMP, RTSP or MMS) cannot go through a proxy. Add its
  server to **Not for these sites (fetched directly)**, or turn the proxy
  off, then click **Retry**.

Torrents and proxies are under "Torrents" below.

## The disk and room

- `disk: …`
  Windows refused to read or write a file; its words follow. If they say
  the disk is full, free some space, or click **Choose another folder…** in
  the download's window. If they say access is denied, choose another
  folder with **Move/Rename**, or in **Options › Folders** for every
  download. Then click **Retry**.
- `it is not there`
- `dm is not allowed to use it`
- `something of that name is already there`
  Windows' file errors in plain words, after the name of the file they are
  about (for example "… could not be read: it is not there"). The first:
  the file or its drive is gone. The second: choose a file or folder dm may
  use. The third: choose another name or folder with **Move/Rename**, or
  remove the other file.
- `the folder … cannot be written to (…); choose another for this download (Move/Rename) or for every download (Options › Folders)`
  dm checks the download folder before fetching anything, and it cannot
  make files there: a removed disk, a folder you may not write to. Do what
  the sentence says, then click **Retry**.
- `not enough room: the file needs …, and … has … free…`
  dm checked the space before fetching, and there is not enough. Where
  other downloads under way will need some of it, the sentence says how
  much. Free some space, or choose a folder on another drive with
  **Move/Rename**, then click **Retry**.
- `too large for this drive: the file is …, and … is on a FAT32 drive, which holds no file over 4 GB; choose a folder on another drive`
  The drive's format cannot hold a file this big. Choose a folder on
  another drive.
- `stopped: the disk has less than … free and this download has no length to say when it ends`
  The server did not say how big the file is, and the disk got close to
  full, so dm stopped before it filled. What came is kept. Free some space,
  then click **Retry**.
- `ten thousand files named … already in …; nothing will be overwritten`
  dm gives a file a numbered name when its name is taken, and every
  numbered name up to ten thousand is taken too. Clear out that folder, or
  choose another with **Move/Rename**.
- `the file could not be cut to size: …`
  Putting the finished file in place, dm could not trim it to its length.
  Windows' words follow. Click **Retry**.
- `a staged part went missing while the file was being joined`
  The fetched part was gone from dm's parts folder while dm copied it to
  the download folder on another drive. Something else removed it, or the
  drive went away. Click **Retry**.
- `the part of range … could not be adopted: …`
  Going on with a download, dm could not take in a part fetched in an
  earlier run. Windows' words follow. Click **Retry**; if it fails again,
  **Redownload** starts the file from the beginning.

## Lists (Import and Export)

- `No file picker here: type the path.`
  Shown in "Import a list" or "Export the list" where the computer has no
  file panel to choose the list's file with (a Linux desktop without one).
  Type the file's full path under **File's path** and click **OK**.
- `Type the file's full path, such as ….`
  The path typed is not a full one (from the drive or `/`, or `~` for your
  home folder). Type it whole, as the example shows.
- `… could not be read: …`
- `… could not be written: …`
  Import could not read the list, or Export could not write it; the file's
  error follows in plain words. Check the path, or choose a folder dm may
  write to, and click **OK** again: the window stays open with the path.

- `the saved queue could not be read (…); it was kept as … and the queue starts empty`
- `the saved queue could not be read (…) or moved aside (…); it will be overwritten by the next change`
  Shown when dm starts and its own list of downloads (`queue.json`) is
  damaged. In the first case the damaged file is kept under the name given,
  so nothing is lost, and dm starts with an empty list. In the second, dm
  could not even move it aside: copy `queue.json` somewhere safe before
  adding a download.

## Backups (Back up dm and Restore)

- `The two passphrases differ.`
  In "Back up dm", **Passphrase** and **Passphrase again** are not the same.
  Type the passphrase again in both.
- `That file is not a backup of dm.`
  The file chosen in **File › Restore…** is not one **File › Back up dm…**
  wrote (a settings file or a list is opened with Options' or Tasks'
  Import). Nothing was restored.
- `That passphrase does not open this backup's sign-ins.`
  The passphrase is not the one the backup was made with (or the file was
  changed since). The window stays open and nothing is restored: type it
  again, with its capital letters, or leave it empty to restore everything
  but the sign-ins.
- `This backup's sign-ins were sealed by a newer dm, which this one cannot open.`
  Update dm, or leave the passphrase empty to restore everything else.
- `… could not be read: …`
- `… could not be written: …`
  The backup's file could not be opened, or written where you chose; the
  file's error follows in plain words.
- `The programs dm runs (after a download, at the end of the scheduled hours, the proxy script) were not restored: set them in Options.`
  A file never sets a program for dm to run. Set them again in Options if
  you want them.
- `Some of its settings could not be read, so their defaults are in use: ….`
  The backup was made by a newer dm, or was changed; the settings named
  keep their defaults, and the rest were restored.
- `Downloads it holds that this dm cannot read, left out: ….`
  A newer dm's downloads this one does not know how to read; the rest were
  added.

## Checksums

A checksum is a fingerprint of a file. dm checks the finished file against
one when it has one: given with the download, listed in a Metalink, sent by
the server, or published in a checksum file beside the file.

- `… was …, but … was published`
  The fingerprint of the file that arrived (for example its SHA-256) is not
  the one published. dm removed the file, since it is known to be wrong.
  Click **Retry**. If it fails again, the published checksum may be for
  another version of the file; tell the site.
- `…; and the file could not be removed from …: …`
  As above, and dm could not remove the wrong file: it is still at the
  place named. Do not open it; delete it yourself.

For torrents, see "Torrents" below.

## Web pages instead of files, and links that expired

A link from a browser often works only for a while, or only once you are
signed in. When such a link fails with a refusal (403, 410) or a web page,
dm keeps what it has and the Status says "this link has expired. dm is
waiting for the browser to hand over a new one for the same file". Start the
download again from the page in your browser: dm gives the new link to the
waiting row, which goes on from what it has. **Update download link** on the
row's right-click menu takes a new address by hand.

- `that link answered with a web page rather than the file — it may have expired, or need a sign-in`
  The address looks like a file, but the server sent a web page, usually a
  sign-in page or one saying the link has expired. Open the page the file
  came from in your browser, sign in if it asks, and download again. A
  login for the site can also go in **Properties › Login**.
- `the stream answered with a web page rather than the file`
  A video stream's address sent a web page instead. The address has
  usually expired. Play the video again in your browser and click
  **Download video**.
- `that address answered with a web page rather than the file; its video, if any, comes from the browser's Download video button when the page plays`
- `that is a web page; its video, if any, comes from the browser's Download video button when the page plays`
  The address is a web page, not a file. When an address you add turns
  out to be a page, dm asks whether to save the page; this is said when it
  is not saved. dm reads YouTube's pages itself; on any other site, play the video
  in your browser and click **Download video** on it.
- `this download is a form's answer, and the form is gone since dm was closed: start it again from the page it came from`
  The file was the answer to a form sent from a page, and what the form
  sent is not kept once dm closes. Fill in the form again on the page.
- `a file of the same name and size is already in the list or in the folder, so it was not fetched again`
  The download is the same file as a finished download or a file in its
  folder, and dm asked **The same file is already here** (see [Questions
  dm asks](questions.md)). Retry asks again; **Download again** there
  downloads it.

## Mirrors and Metalinks

A Metalink lists the same file on several servers (mirrors).

- `that Metalink names no file dm can fetch over HTTP`
  The Metalink lists no web (HTTP or HTTPS) address for its file.
- `no mirror in that Metalink has the file it describes: every one that answered disagreed about its length`
  Every mirror that answered has a file of another size than the Metalink
  says. The mirrors may not have the new version yet. Try again later.
- `not one mirror in that Metalink answered`
  No mirror could be reached. Check that you are online, or try later.

## Video pages and YouTube

dm reads YouTube's pages itself. When it cannot get a video, the Status says
"dm couldn't get the video from this page", with dm's reason after it.
YouTube changes its pages from time to time: try again later, and look for
a new dm with **Help › Check for updates**.

- `YouTube says: …`
  YouTube refused the video and gave its reason: private, removed, not
  shown in your country, or a check that you are not a bot. For the bot
  check, play the video in your browser, then click **Download video** on
  it.
- `not a YouTube video address`
- `not a YouTube playlist address`
  The address is not one of a YouTube video, or of a playlist or channel.
  Check the address.
- `the page took too long to read`
- `the playlist took too long to read`
  YouTube did not answer within dm's time limit. Click **Retry**.
- `the page is larger than any YouTube page`
  What came back was far larger than a YouTube page is, so dm stopped
  reading it. Click **Retry**.
- `the player API answered no JSON (…)`
- `the page names no player script`
- `the player names no stream dm can fetch`
- `a cipher with no address`
- `a cipher with no signature`
- `the signature was not solved`
- `a stream with no address`
- `a stream's address is not one (…)`
- `YouTube's checks were not answered: …`
  YouTube's answer was not what dm expected, or dm could not work out the
  checks YouTube's player sets for its stream addresses. Try again later,
  and check for a new dm.
- `no client to ask YouTube's player as`
  A check inside dm. It should not happen; use **Copy details** and report
  it.
- `no stream at that quality`
  YouTube has no stream at the quality chosen. Choose another quality.
- `a live stream`
  The video is live, and YouTube's player named no live playlist for dm to
  record from.
- `the premiere starts at …, more than a week from now`
  The premiere, or the live stream, is set to start at that time, on your
  computer's clock: too far ahead for dm to wait for. Add the video again
  nearer the time.
- `the premiere had not begun six hours after the time it was set for`
  dm waited for the premiere and read the video every half minute, but it
  did not begin within six hours of its time. It may have been put off or
  cancelled: check the video in your browser, then click **Retry**.
- `dm's own reading of the page could not be read (…)`
- `this video's streams cannot be fetched by dm's own engine`
  dm read the page but its streams are not ones dm can fetch. Try again
  later; if it stays, use **Copy details** and report it.
- `the video was fetched but the file it named is missing`
  The picture and sound came, but the joined file was not there at the end.
  Click **Retry**.
- `the page names no captions`
- `the video has no … captions now`
- `the captions have no address`
  The caption track you chose is not on the video now, in that language.
  Open the video's list again and choose a track it shows.
- `the channel's page names no channel (removed, or not a channel)`
- `the playlist page holds no list (private, removed, or a sign-in wanted)`
- `the playlist page lists no videos (private, removed, or a sign-in wanted)`
  The playlist or channel could not be read for the reasons given. Check it
  opens in your browser.
- `that address is not an RSS or Atom feed`
  The address looked like a feed's (it ends in `.rss`, or is named `feed`),
  but what came back is not a feed. Check the address in your browser.
- `the feed took too long to read`
  The feed's server did not answer within dm's time limit. Try again later.
- `the feed is larger than any feed`
  What came back was far larger than a feed is, so dm stopped reading it.

## Parts of a video ("Only a part of the video")

**Only a part of the video** in **Download details** fetches the stretch
between two times.

- `a part can be cut only from a video page, an HLS or DASH stream or an MP4 file; this address is another kind of file`
- `a part of a file can be cut only from a server that serves byte ranges and says how long the file is`
- `this stream carries no index of its pieces, so a part of it cannot be cut: only a video whose streams do (YouTube's MP4) can`
- `this video's streams are not plain files with an index, so a part of it cannot be cut`
- `this video's streams are numbered pieces with no index, so a part of it cannot be cut`
- `this file has no index (moov) to cut a part by`
  A part cannot be cut from this video, for the reason given. Download the
  whole video instead.
- `a part is cut from MP4 streams only, not WebM`
  Choose an MP4 quality for the part.
- `the video has nothing between … and …`
- `that part is outside the stream's length`
- `that part is outside the stream, or the stream does not say how long its pieces are`
  The times asked for are past the end of the video. Check the part's
  start and end times.
- `this file's boxes do not add up, so no part can be cut`
- `this file's index is too large`
- `this stream's index does not add up`
  The video's index, which says where each moment is, could not be used.
  Download the whole video instead.
- `the part could not be built: …`
- `the part could not be cut: …`
  The part came, but making the file from it failed; the reason follows.
  Click **Retry**.
- `reading the index stopped: …`
- `building the part stopped: …`
- `no part was asked for`
  Checks inside dm. They should not happen; click **Retry**.
- `the MP3 could not be made: …`
- `the GIF could not be made: …`
- `the MP3 could not be written: …`
- `making the MP3 stopped: …`
- `the GIF could not be written: …`
- `making the GIF stopped: …`
  The sound or the part came, but turning it into an MP3 or a GIF failed;
  the reason follows. The file it was made from is not removed.

## Streams (HLS, DASH, encryption)

A stream is a video cut into many small pieces, listed in a playlist (HLS)
or a manifest (DASH). These failures mean the stream is not one dm can
fetch, or the server sent something other than it listed. Try again; if
the address came from your browser, play the video again and click
**Download video**, which hands dm fresh addresses.

- `that is not an HLS playlist`
- `no playable rendition in that stream`
- `that playlist's rendition is another master playlist, not one of segments`
- `that playlist lists no media`
- `that playlist listed no segments`
  The playlist does not list pieces dm can fetch.
- `…: this stream shape is not supported yet`
  The stream is built in a way dm cannot fetch yet; the reason comes first
  (for example, its first piece changes part-way through, or is
  encrypted).
- `a stream listed an address dm cannot read: …`
- `the stream names a byte range dm cannot read: …`
- `a byte range with no start follows nothing`
- `a byte range of no length`
- `the initialisation segment's byte range has no start`
  The playlist is written wrongly.
- `… did not honour the byte range this stream is cut by`
- `… answered a byte range with the wrong number of bytes`
  The server did not send the piece of the file the playlist asked for.
- `… is far larger than a stream's segment should be`
- `… is larger than anything of its kind should be`
  Something the stream named (a piece, a playlist, a key, a Metalink) kept
  coming far past any real one, so dm stopped it.
- `this stream uses … encryption, which dm cannot decrypt`
  The stream is copy-protected in a way only the site's own player can
  undo. dm cannot fetch it.
- `the stream is encrypted but names no key`
- `a stream key must be 16 bytes; this one is …`
- `that stream key is not usable`
- `a stream segment failed to decrypt`
  The stream is encrypted, and its key is missing or wrong. The key's
  address may have expired: play the video again in your browser and click
  **Download video**.
- `a scrambled stream's segments went missing before they were joined`
- `clearing stopped: …`
- `this stream's samples are scrambled (SAMPLE-AES) and could not be put in the clear: …`
  The stream's samples are scrambled with a key the site hands out, and dm
  could not unscramble what it fetched. Download it again; if it fails the
  same way, the stream is in a shape dm does not handle yet.
- `this stream's sound is scrambled (SAMPLE-AES) in packed audio, which dm does not put in the clear yet`
  dm cannot unscramble this kind of stream's sound yet. Choose another
  quality or format if the page offers one.
- `that is not a DASH manifest dm can read: …`
- `a manifest listed an address dm cannot read: …`
- `a manifest's … is not one dm can read`
- `a manifest's periods are not one dm can read (far too many pieces)`
- `that manifest lists nothing to download`
- `a manifest's periods could not be joined`
  The DASH manifest is not one dm can use, for the reason given.
- `this DASH stream is live, so it is recorded as it goes rather than planned whole`
  dm records a live DASH stream as it goes; this is said only if one is
  taken for a finished video. Click **Retry**.
- `… does not serve byte ranges, which a picture and sound fetched apart need`
- `… did not say how long it is`
- `… says it is … bytes long, more than a video's picture or sound is`
  The video's picture and sound come from separate addresses, and this one
  did not answer as dm needs to fetch it in parts.
- `the picture and sound could not be joined: …`
  Both came, but making one video of them failed; the reason follows. Click
  **Retry**. Where the disk had too little room for the usual join, dm joins
  the two in the larger track's own file, and the reason can be one of its
  steps: `… could not be opened: …`, `… could not be written: …`, `an input
  could not be shortened: …`, or `the disk has no room for the … bytes it
  adds`. The tracks it had begun to change are fetched again on **Retry**.
- `joining stopped: …`
  The join in the track's own file ended before its end (dm was closed, or
  the system stopped it). **Retry** fetches the changed tracks again.
- `the live stream sent nothing to record`
- `the live recording could not be joined: …`
  A live DASH recording got nothing, or what it got could not be made into
  a video.

- `the stream could not be put into an MP4: …`
  Every piece of the stream came, but they could not be joined into one
  MP4, and the pieces were already let go as they were joined. The reason
  follows. Click **Retry** to fetch the stream again.

## Live recording (RTMP, RTSP, MMS)

dm records `rtmp://`, `rtmps://`, `rtmpt://`, `rtsp://`, `rtsps://` and
`mms://` streams. When a stream stops after something was recorded, what
was recorded is kept.

- `…:// is an encrypted stream dm cannot record`
  RTMPE is Flash's encrypted RTMP, which dm does not undo.
- `… did not answer`
- `the RTSP server did not answer`
  The server did not answer within its time. Check the address and that you
  are online.
- `the server sent nothing for 30 seconds`
- `the RTSP server sent nothing for 30 seconds`
- `the MMS server sent nothing for 30 seconds`
  The connection opened, but no stream came. The stream may not be on air
  now.
- `the server ended the stream before sending any of it`
- `the RTSP server ended the stream before sending any of it`
- `the MMS server ended the stream before sending any of it`
  The server closed the stream before any of it came. Try again when it is
  on air.
- `the server would not play it: …`
  The RTMP server refused the stream; its reason follows. Check the
  address.
- `the server does not speak plain RTMP`
- `the server sent a message too large`
  The server is not an RTMP server dm can record from.
- `not an rtmp:// address`
- `dm does not speak …://`
- `the address names no application and stream`
  The address is not one dm can record over RTMP: it must start with
  `rtmp://` (or `rtmps://`, `rtmpt://`) and name the server's application
  and the stream, as `rtmp://server/app/stream`. Check the address.
- `RTMPT: …`
- `… did not open an RTMPT session: …`
- `… answered no RTMPT session`
  RTMPT carries RTMP over the web, and the server did not open it.
- `the RTSP server: …`
- `the address names no server`
- `the rtsps:// address could not be read`
- `the RTSP stream has no H.264, H.265 or AAC dm can record`
- `the RTSP stream never said what its picture or sound is`
  The RTSP server refused, or its stream is in a form dm cannot record.
  Check the address, and a login in it if the camera or server wants one.
- `not an mms:// address`
- `the MMS server: …`
- `the MMS server answered …`
- `the MMS server sent no stream header`
- `the MMS stream names no streams`
  The MMS server refused or sent no stream dm can record.

## FTP

- `that is not an FTP address: …`
- `that FTP address names no server`
- `that FTP address is a folder, not a file`
- `that FTP address has a line break in it`
  The address is not one dm can download from. Check it; a folder's address
  ends in `/`.
- `the FTP server refused the sign-in (…) — put the name and password in the address: ftp:// followed by name:password@ and the server`
  The server wants a name and password. Write them in the address, as the
  sentence says.
- `the FTP server has no file … (…)`
  The file is not on the server at that path. Check the address.
- `the FTP server said …`
  The server said "try later" (a 4xx answer: too many users, say). dm tries
  again by itself.
- `the FTP server answered …`
  The server refused, with its code and words.
- `the FTP server would not list the folder`
  The server would not send the folder's contents.
- `… is not a name TLS can check: …`
- `the FTP server would not speak TLS (…)`
- `the FTP server would not keep the data in TLS (…)`
- `the FTP server's TLS failed: …`
  An `ftps://` address asks for an encrypted connection, and the server did
  not give one. dm does not fall back to an unencrypted one.
- `the FTP server's reply ran on`
- `the FTP server answered something that is not FTP: …`
- `the FTP server's passive reply named no port`
  The server's answers are not ones dm can follow. Check that the address
  is an FTP server's.

## Torrents

- `this file is not a torrent: …`
- `could not read the torrent file …: …`
- `that .torrent file is implausibly large`
  The `.torrent` file is damaged, is not a torrent, or (over 10 MB) is far
  larger than any real one. Get it again from where it came from.
- `the torrent engine would not start: …`
- `the torrent could not be added: …`
- `the torrent produced no handle`
- `the torrent's next files could not be asked for: …`
- `the finished torrent has no metadata: …`
- `the finished torrent could not be released: …`
  dm's torrent engine failed; the reason follows. Click **Retry**, or close
  and open dm again.
- `its session would not start: …`
- `it could not be added again: …`
- `it produced no handle`
- `its files are no longer whole`
  Sharing a finished torrent with others stopped. In the first three, dm's
  torrent engine would not take the torrent up again; the reason follows.
  In the last, the files on the disk no longer match the torrent (one was
  moved, changed or deleted), so dm stopped sharing them. What was
  downloaded is not touched.
- `the torrent names a file outside its own folder, so it was not downloaded`
  A file name in the torrent would land outside the torrent's folder, so dm
  fetched nothing of it.
- `a checksum cannot be checked over a torrent of several files`
  A checksum fits one file, and this torrent has several. Download the
  torrent without a checksum.
- `the peer blocklist at … could not be read: …`
  The list set in **Options › Advanced** under **Block the peers listed at
  this address (optional)** could not be read, so the torrent stopped.
  Check the address, or clear it.
- `behind a proxy dm finds peers only through http or https trackers, and this torrent lists none`
  With a proxy on, dm finds peers only through web trackers, and this
  torrent has none. Turn the proxy off for it, or use a torrent with web
  trackers.
- `a torrent can only go through a SOCKS5 proxy; the configured proxy is not one, and BitTorrent will not go around it`
  Choose a SOCKS5 proxy in **Options › Advanced**, or turn the proxy off.
- `torrents are held off the network in this run (DM_NO_SWARM)`
  Seen only in dm's own tests, which keep torrents off the network. If you
  see it, dm was started with DM_NO_SWARM set: start it without.

## Archives (extracting)

**Extract here** and **Extract to…** on a finished archive's right-click
menu put its files in a new folder beside it. dm checks every entry before
it writes anything; if one fails, nothing is extracted, the new folder is
removed, and the archive stays where it was.

- `this is not an archive that can be read: …`
  The file is damaged, or not an archive dm reads.
- `the archive has an entry that would leave its folder: …`
  An entry's name would put a file outside the new folder. dm refuses the
  archive; it may be made to do harm.
- `the archive holds a link or special file, which dm does not extract: …`
- `the archive is password-protected, which dm does not extract: …`
  The archive is locked with a password. The window does not ask for one
  yet; unpack it with another program that asks.
- `wrong password for …`
  The password given does not open the archive (or the locked part of it is
  damaged, which looks the same). Nothing was written. Check the password,
  including its capital letters.
- `the archive is packed with …, which dm does not unpack`
  The archive needs a program that handles these; dm does not. For a RAR,
  the words name what: `the compression of RAR 4 and older` (dm unpacks
  RAR 5 archives whole, and of an older RAR archive only the files it
  stores without compression), `a later RAR compression (version …)`, or
  `BLAKE2 checksums in place of CRC32` (the files are checked by a hash dm
  does not compute, so dm does not write them unchecked).
- `this is not an archive that can be read: the RAR archive is one part of several (.part1.rar, .part2.rar…), which dm does not join`
  The archive was split into parts. Unpack it with a program that joins
  RAR parts, once every part is downloaded.
- `the archive lists the same name twice: …`
  Two entries would be one file on a Windows disk.
- `the archive is larger than dm will extract (…)`
  It would make too many files, too many bytes (over 64 GB), or far more
  than any real archive packs into its size.
- `the archive was stopped`
  The extraction was stopped.
