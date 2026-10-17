# The "Download video" panel

The browser extension shows a small panel over a video player so a person can
hand the video to dm. This page lists what the panel does and which test holds
each line. The panel carries dm's own mark and colours.

| #  | Behaviour                                                                                                   | Test |
|----|-------------------------------------------------------------------------------------------------------------|------|
| 1  | Shown when playback starts and while the pointer is on the player, through any overlay; kept while the pointer rests | `chrome-check.py`, `youtube-check.py` |
| 2  | One compact bar on the player's top-right corner: dm's mark, "Download video", a ▾ when there is a choice, ? and × | `decide.rs` placement tests; `chrome-check.py` reads its place |
| 3  | A file `<video src>` is one quality: a click hands it over at once                                          | `chrome-check.py` |
| 4  | A player page (YouTube, any MediaSource player): the app asks its yt-dlp (`yt-dlp -J`) and the panel lists one line per height, tallest first, with the size, then the sound alone | `ytdlp.rs` `a_pages_formats_become_one_choice_per_height_and_the_sound_alone`; `host.rs`; `chrome-check.py` with a stand-in yt-dlp |
| 5  | A chosen line goes to the app with its quality and container, and the app opens "Download details" for it (name, folder, note, queue) before starting; Options › General can turn that off | `window.rs` |
| 6  | The app downloads it like any other row; the page can be closed                                             | the queue's own tests |
| 7  | × opens "Hide for now" (that player, until the page reloads) and "Never on <site>" (every page of the site, and its subdomains, until the toolbar popup turns it back on, which takes effect without a reload); ? says what the panel does | `decide.rs` site tests; `chrome-check.py` |
| 8  | A site whose security policy forbids WebAssembly gets no panel; its right-click menu still works             | `chrome-check.py` |
| 9  | Videos on sites yt-dlp does not know: the HLS and DASH playlists and whole audio and video files the page's player loaded (seen in its network responses, never changed) are listed beside yt-dlp's qualities, or alone; segments and partial ranges are not | `decide.rs` stream tests; `chrome-check.py` stream pages; `firefox-check.py` |
| 10 | A player smaller than the panel (a 96px clip) gets none                                                     | `chrome-check.py` |
