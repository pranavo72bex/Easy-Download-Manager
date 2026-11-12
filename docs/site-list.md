# Sites to check in the human QA, beside reference A

The maintainer's list (2026-11-05) of popular sites where reference A works: dm should work on
each as reference A does. Reference A keeps no list of sites; it takes downloads and media from any
server through its browser extension, and so does dm. These are the sites a person checks in the
final human QA, with both apps in Microsoft Edge on Windows, as `docs/reference-behavior.md`
describes (`DM_KEEP_OTHER_EXTENSIONS=1`, `SITES_HOLD`, `scripts/sites-check.py` for the video
sites it knows).

**For each site:** open a public page, do what the "Try" column says in Edge with reference A's
extension, then with dm's, and write in the last two columns what each did (the file, its name
and size, whether it is whole; a video's qualities and whether picture and sound arrived). A
difference is a row in `docs/gaps.md`.

**The rules of the run:**

* Nobody's account is used but the maintainer's, signed in by the maintainer (gaps rows 62, 63).
* Only what the person running it may download: a public file, a Creative Commons or public-domain
  video, the person's own upload.
* A stream protected by a DRM system (Widevine, PlayReady, FairPlay; most paid course platforms
  and paid music) is refused by both apps, which decrypt none; dm says so in words (row 131,
  `browser/src/decide/protection.rs`). That is the expected result there, not a gap.
* A site that is gone, or answers an automated browser with a challenge, is written down as that.

**How dm gets it**, in the tables: *panel* is the button over a playing video, from the
player's own requests (or dm's own YouTube reader); *hand-over* is the browser's download taken
by the extension (a host's Download button); *file* is a direct link, also by Add URL.

Status: *not yet* until a person has run it beside reference A; where an earlier run covered a
site, the row says which.

## Video and media streaming

| Site | Try | Sign-in | How dm gets it | Reference A | dm |
|---|---|---|---|---|---|
| YouTube | a public video; a Short; a playlist | no | panel (dm's own reader) | run 2026-11-04 (row 142) | whole, every quality but 60 fps lines (row 72d) |
| Vimeo | a public video | no | panel | run 2026-11-04 (row 142) | whole with sound (row 142) |
| Dailymotion | a public video | no | panel | not yet | 2026-11-07, `video-sites-check.py` in Edge on Windows: not judged; the player is a frame of Dailymotion's own that the check lost hold of twice, and the player page alone showed no video to an automated browser. 2026-11-09 on macOS, signed out: the panel shows on the player and its list stays empty; the extension saw no stream request from the player's frame while it played, and dm's own reading of the page finds no qualities. The player's public metadata (`/player/metadata/video/<id>`) names an HLS playlist, which answered a program 403 (`x-error-code: E005`); what the player sends that a program does not was not looked into further (the check stopped there). For the run beside reference A |
| Twitch | a past broadcast and a clip | no | panel (HLS) | not yet | 2026-11-07, `video-sites-check.py` in Edge on Windows, no sign-in: a NASA highlight (HLS) and a NASA clip, each a whole video with sound; a whole past broadcast is hours long and was still arriving when the check stopped waiting |
| Facebook | a public video post, a reel | maintainer | panel | run 2026-11-04 (row 142); signed in on Windows 2026-11-07 | whole with sound: a NASA video post (26 s, 1280 tall) and the Watch page (222 s); saved as `video.mp4`, the page's title being "(1) Facebook" (row 75) |
| Instagram | a reel | no | panel | run 2026-11-04 (row 142) | whole; one run in five a sign-in layer hides the button (row 71) |
| TikTok | a video | no | panel | run 2026-11-04 (row 142) | whole with sound (row 71) |
| X (Twitter) | a post with a video | no | panel | run 2026-11-04 (row 142) | whole (row 142) |
| Reddit | a post with a video | maintainer | panel (plain-JavaScript panel: Reddit forbids WebAssembly) | run 2026-11-04 (row 142) | whole with sound (rows 63, 142) |
| Rumble | a public video | no | panel | not yet | 2026-11-07: Rumble answered the automated Edge with Cloudflare's "Just a moment…" check; not judged |
| Bilibili | a public video | no | panel (picture and sound as two whole MP4 files the player fetches in ranges, joined by dm) | not yet | 2026-11-07, Edge on macOS, signed out: a whole 480p video with sound, 65.9 s (gaps row 155, closed: the panel tells the two files apart by their first bytes, `decide::mp4_track`) |
| Metacafe | a video | no | panel | not yet; check the site is still running | not yet |
| Veoh | a video | no | panel | not yet; check the site is still running | not yet |
| Break | a video | no | panel | not yet; check the site is still running | not yet |
| LiveLeak | — | — | — | reported closed in 2021: confirm and record | — |

## File hosting and cloud storage

A share link's page, then its own Download button: the host's page starts the real download in
the browser, which the extension hands to dm (a pasted page address alone is refused as a web
page, row 136). Many hosts make a visitor wait, solve a check or sign in first; that is written
down, not worked around.

| Site | Try | Sign-in | How dm gets it | Reference A | dm |
|---|---|---|---|---|---|
| Google Drive | a public file's share link (and one over 100 MB, which asks to confirm) | no | hand-over | not yet | not yet |
| OneDrive | a public share link | no | hand-over | not yet | not yet |
| Dropbox | a public share link (`?dl=0` page, and `?dl=1`) | no | hand-over; `?dl=1` also as a file | not yet | not yet |
| MediaFire | a public file | no | hand-over | not yet | not yet |
| MEGA | a public file link (decrypted in the browser by MEGA's page) | no | hand-over | not yet | not yet |
| Box | a public share link | no | hand-over | not yet | not yet |
| iCloud Drive | a shared file link | no | hand-over | not yet | not yet |
| WeTransfer | a transfer link the tester made | no | hand-over | not yet | not yet |
| SendSpace | a public file | no | hand-over | not yet | not yet |
| 4shared | a public file | no | hand-over | not yet | not yet |
| Rapidgator | a free download | no | hand-over | not yet | not yet |
| 1fichier | a free download | no | hand-over | not yet | not yet |
| Turbobit | a free download | no | hand-over | not yet | not yet |
| Nitroflare | a free download | no | hand-over | not yet | not yet |
| MegaUp | a free download | no | hand-over | not yet | not yet |
| Uploaded.net | a free download | no | hand-over | not yet; check the site is still running | not yet |
| Uptobox | a free download | no | hand-over | not yet; check the site is still running | not yet |
| Zippyshare | — | — | — | reported closed in 2023: confirm and record | — |

## Software and open-source repositories

| Site | Try | Sign-in | How dm gets it | Reference A | dm |
|---|---|---|---|---|---|
| GitHub | a release asset; a repository's "Download ZIP" | no | file, hand-over | release asset run 2026-11-04 (row 132) | byte-identical (rows 132, `real-sites.py`); 2026-11-06 `handover-check.py`: a release asset's link and the Code menu's Download ZIP (a `codeload` address named by the server) each handed over and arriving |
| GitLab | a release asset; an archive | no | file, hand-over | not yet | not yet |
| Bitbucket | a repository's download | no | hand-over | not yet | not yet |
| SourceForge | a project's Download button (the page counts down, then starts) | no | hand-over | not yet | 2026-11-06 `handover-check.py`: the countdown page's download (to a mirror) handed over and whole |
| Microsoft Download Center | a public download | no | hand-over | not yet | not yet |
| Apple Support Downloads | a public download | no | hand-over | not yet | not yet |
| FileHippo | a program's download | no | hand-over | not yet | not yet |
| CNET Download | a program's download | no | hand-over | not yet | not yet |
| MajorGeeks | a program's download | no | hand-over | not yet | not yet |
| Softpedia | a program's download | no | hand-over | not yet | not yet |
| TechSpot | a program's download | no | hand-over | not yet | not yet |
| Ninite | an installer built on the site | no | hand-over | not yet | not yet |

## Education and e-learning

Course videos on these sites are behind a sign-in, and on most of them protected by a DRM system:
expected, both apps refuse a protected stream (see the rules above). What is checked is the free,
public material (a course's preview, an open lecture, a downloadable resource) and that a refusal
says why.

| Site | Try | Sign-in | How dm gets it | Reference A | dm |
|---|---|---|---|---|---|
| Udemy | a course's free preview; an attached resource | maintainer | panel, hand-over | not yet | not yet |
| Coursera | a free lecture video; a lecture's slides | maintainer | panel, hand-over | not yet | not yet |
| edX | a free lecture video | maintainer | panel | not yet | not yet |
| Khan Academy | a lesson video | no | panel (YouTube underneath) | not yet | not yet |
| LinkedIn Learning | a free preview | maintainer | panel | not yet | not yet |
| Skillshare | a free lesson | maintainer | panel | not yet | not yet |
| Pluralsight | a free preview | maintainer | panel | not yet | not yet |
| Udacity | a free lesson | maintainer | panel | not yet | not yet |
| MasterClass | a trailer | no | panel | not yet | not yet |

## Music and audio

| Site | Try | Sign-in | How dm gets it | Reference A | dm |
|---|---|---|---|---|---|
| SoundCloud | a track the artist allows to be downloaded (its Download button), and a stream | no | hand-over; panel for the stream | not yet | not yet |
| Bandcamp | a free or name-your-price download; a track's stream | no | hand-over; panel | not yet | not yet |
| Mixcloud | a mix's stream | no | panel | not yet | not yet |
| Audiomack | a track's stream, a free download | no | panel, hand-over | not yet | not yet |
| ReverbNation | a free download | no | hand-over | not yet | not yet |

## Stock images and vectors

| Site | Try | Sign-in | How dm gets it | Reference A | dm |
|---|---|---|---|---|---|
| Unsplash | a photo's Download button | no | hand-over | not yet | not yet |
| Pexels | a photo's and a video's Download | no | hand-over | not yet | not yet |
| Pixabay | a free image's Download | no | hand-over | not yet | not yet |
| Freepik | a free vector's Download | maintainer | hand-over | not yet | not yet |
| VectorStock | a free vector's Download | maintainer | hand-over | not yet | not yet |

## Sites for testing the engine

| Site | Try | Sign-in | How dm gets it | Reference A | dm |
|---|---|---|---|---|---|
| Ubuntu ISO mirror | a current ISO (several GB: many connections, pause and resume, a kill and a restart) | no | file, hand-over | not yet | not yet |
| VLC media player | the Windows installer from the official page (a mirror redirect) | no | hand-over | not yet | not yet |
| Test-Videos.co.uk | an MP4 and a WebM of each size | no | file, panel | not yet | 2026-11-07 (Add URL, Windows): the 360p 10 s MP4 (H.264) and WebM (VP9), each byte-identical to a plain fetch and whole |
| NASA Image and Video Library | a video and an original image | no | file, panel | not yet | 2026-11-07 (Add URL, Windows): a library video's mobile MP4 (7.6 MB) and its original (221 MB), whole and the size the server gives |

## Download pages, run by `scripts/handover-check.py` (2026-11-06, Microsoft Edge on macOS)

A page's own Download button, clicked as a person clicks it (or nothing, where the page starts
the download itself), with the app's catch rules as a new install has them. Each was handed to dm
and arriving, and the browser kept nothing:

| Page | How it starts the download | dm |
|---|---|---|
| 7-zip.org | a plain link (to a GitHub release asset, signed redirect) | whole |
| python.org | a button's link | arriving |
| nodejs.org | a button's link | arriving |
| GitHub releases (cli/cli) | an asset's link, signed redirect to another host | arriving |
| GitHub Download ZIP | a link in the Code menu; the address names no file, the server does | whole |
| SourceForge | a countdown page, then a mirror | whole |
| mozilla.org (thank-you page) | started by the page's script, to the CDN | arriving |
| get.videolan.org | a page that refreshes to a mirror | whole |
| audacityteam.org | a link to a GitHub release asset | whole |
| gimp.org | a link to a mirror | arriving |
| code.visualstudio.com (`?dv=`) | started by the page's script, redirect to Microsoft's CDN | arriving |
| libreoffice.org | the page's script opens the link in a new tab and sends the page to its donate page; a redirect to a mirror | whole (2026-11-11, Chrome for Testing on macOS) |

LibreOffice failed until 2026-11-11 (gaps row 173) because the check's click missed the link: the
page's menu bar grows back by 63 pixels when the page is scrolled near its top, after the check
had measured where the link was, so the click landed on the heading above. The check now
measures the link where it rests after one scroll. The whole check, 2026-11-11 on macOS: 68 of 68,
all twelve pages handed to dm and arriving.

