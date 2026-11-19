# Videos and their qualities

dm downloads a video from the page it plays on, at the quality you choose.

## From the browser: Download video

Move the mouse over a playing video and a small bar shows on its top-right
corner: **Download video**. Click it to hand the video to dm; where there is
a choice, its ▾ lists each quality first. The grip at the bar's left drags
it out of the way, and × offers to hide it for that video, or never to show
it on that site. **Options › Browser › Show Download video on web videos**
turns it off everywhere.

- On YouTube the list shows one line per height, tallest first, each with
  its size, then the sound alone. A height offered at two frame rates is two
  lines ("720p HD 60 fps" and "720p HD"). Each caption track is a line of its
  own ("TTML file, EN subtitles"; "(ASR)" marks captions YouTube made by
  speech recognition). **Download all** hands every line over.
- On other sites the list is the streams and files the page's player
  loaded. If nothing is listed, play the video for a few seconds first, so
  the player loads it, and try again.
- You can also right-click a video and choose **Download media with dm**.

With **Options › General › Ask where to save videos** on, [Download
details](download-details.md) opens first, so you can choose the folder,
the name, the quality and a part; off, the video starts at once.

## From an address

Paste a video page's address in [Add URL](add-url.md). Download details
reads the video and shows each quality as a button with its size ("1080p MP4"
over "275.0 MB"; "Sound only" for the sound alone). A YouTube playlist or
channel opens [Playlist](playlist.md) instead, with one quality for every
video.

## Which quality is picked

**Options › Downloads › Video quality** sets the quality picked unless you
choose another:

- **Best available:** the tallest picture on offer.
- **Up to 1080p**, **Up to 720p**, **Up to 480p:** the tallest picture at or
  below that height, for a smaller file.
- **Audio only:** the sound alone, without the picture.

Where the best qualities are copy-protected, dm takes the best one that is
not, and the row says "higher qualities are copy-protected, so the best one
that is not was taken". dm does not break copy protection.

## A part of a video

For a YouTube video, Download details has **Only a part of the video**: give
**from** and **to** as minutes and seconds (such as 4:10 and 4:45) and only
that part is downloaded. See [Download details](download-details.md).

## Subtitles, formats and live streams

- **Options › Downloads › Embed subtitles in videos from player pages:** "A
  video's subtitles are saved inside its file, to turn on in your player."
  It covers a DASH stream's subtitles too, where the stream lists each
  language as one WebVTT file.
- **Options › Downloads › Save streams that arrive as .ts as MP4:**
  "Streamed videos are saved as MP4 files, which more players open."
- A live stream is recorded as it plays, until it ends, you stop it, or
  **Options › Downloads › Stop recording after (minutes, 0 for no limit)**
  is reached.

When a video does not download, its row says why: see the video and stream
sections of [What dm's messages mean](failures.md).
