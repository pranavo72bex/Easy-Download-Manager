# Options › Browser

Which downloads EDM takes from your browser, and what the browser extension
does. See [Options](options.md) for how the window works.

## The extension

**Add EDM to your browser:** lists each browser EDM has an extension for
(Microsoft Edge, Firefox, and Chrome and others). Beside each it says
**Connected** once that browser's extension has spoken to EDM, with "Connected
· last heard … ago"; where it has not, **Add** opens the page to add the
extension. **Check now** looks again, after you add or remove one.

## Magnet links and .torrent files (Windows)

**Magnet links open with:** and **.torrent files open with:** say which
program Windows opens them with: EDM, **another program**, **nothing yet**,
or **the app chosen in Windows’ Default apps**. It says **Looking…** while
it asks Windows.

- **Use EDM for magnet links** and **Use EDM for .torrent files** make EDM the
  program that opens them.
- **Give magnet links back** and **Give .torrent files back** return them to
  the program that had them before.
- "EDM keeps what another program had, and can give it back. Where Windows'
  Default apps has a choice, that choice decides: change it in Settings,
  Apps, Default apps."
- "The first magnet link you click may make Windows ask which app opens it:
  choose EDM there. EDM can't answer for you."

## The right-click menu

**Items EDM adds to the browser’s right-click menu:** turn an item off to take
it out of the browser's right-click menu:

- **Download link with EDM:** the link you right-clicked.
- **Download media with EDM:** the video or sound you right-clicked.
- **Download all links with EDM:** every link on the page, in
  [Download links](download-links.md).
- **Download selected links with EDM:** the links in the part of the page
  you selected.

## Taking downloads from the browser

- **Take these file types from the browser:** EDM takes a download the
  browser starts when its type is one listed below. "Off: the browser keeps
  every download it starts. Its right-click menu still sends links to EDM."
- **File types:** "Separate types with spaces, no dots. Leave empty for
  all." For example `zip exe mp4`.
- **Ignore files smaller than (KB):** a smaller download stays with the
  browser.
- **Take downloads from these browsers:** a switch for Microsoft Edge, Google
  Chrome, Firefox, Brave, Opera, Vivaldi and **Other Chromium browsers**
  (Chromium and its other relatives). A browser switched off keeps its
  downloads.
- **Never take downloads from these sites:** "Separate sites with spaces.
  example.com covers its subdomains."
- **Sites whose links work only once:** "EDM takes their downloads without
  first asking the server whether it can fetch them, which would use the
  link up. A link that needs the browser's sign-in then fails in EDM rather
  than finishing in the browser."

The settings above are greyed while **Take these file types from the
browser** is off.

## Keys held while clicking a link

- **Hold while clicking to leave it with the browser:** **Alt**, **Shift**
  or **Ctrl or Cmd**. Hold it while you click a download link and the
  browser downloads it itself.
- **Hold while clicking to send it to EDM whatever its type or size:**
  **None**, **Alt**, **Shift** or **Ctrl or Cmd**. Hold it and EDM takes the
  download even if its type is not listed or it is too small.
- If both are the same key, EDM warns: "The two keys are the same; holding it
  leaves the download with the browser."

## Videos and sign-ins

- **Show Download video on web videos:** "A button over a video on a web
  page, to download it with EDM." See [Videos and their
  qualities](videos.md).
- **Send the browser’s sign-in with downloads:** "Files that need you signed
  in come to EDM too. Your sign-in goes with that one download and is
  forgotten after it. Off: only the sites you turn on in the extension's
  menu send it." See [Sign-ins](sign-ins.md).
- "Your browser picks this up the next time it sends EDM a download. Its
  right-click menu sends any link either way."
