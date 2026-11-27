# Options › General

How dm looks, starts and talks to you. See [Options](options.md) for how
the window works.

- **Language:** the language of dm's window, from the languages dm has.

## Appearance

- **Theme:** **As the computer is** follows the computer's light or dark
  setting; **Light** and **Dark** keep one. On a phone the first is called
  **As the phone is** and follows the phone's dark theme.
- **Text size:** how large everything in dm's windows is drawn: 60%, 80%,
  100%, 120%, 150% or 200%. Drag the slider or click a percentage. It is used at once,
  in every window, and kept even if you then click Cancel. **View › Larger**
  and **Smaller**, and Ctrl with + and −, step through the same sizes. Where the
  toolbar's buttons no longer fit across the window, those at its end go under
  **More** (three dots) at the right.

## Starting and updating

- **Start dm when I sign in:** dm starts when you sign in to the computer.
- **When I close the window:** what closing dm's window does. **Ask me**
  asks each time, **Keep running in the tray** keeps dm downloading with its
  icon in the tray, **Exit dm** quits.
- **When an address is already in the list:** what happens when you add, or
  the browser sends, an address that is in the list already. **Ask me**
  asks (see [Questions dm asks](questions.md); its **Remember my choice**
  sets this), **Show it** chooses the download that has it, **Download
  again** adds it as a new download, **Replace the old file** downloads it
  again and replaces the old file once the new one is whole.
- **When the same file is already here:** what happens when a download from
  another address is the same file as a finished download or a file in the
  folder (its name and size). **Ask me** asks (see [Questions dm
  asks](questions.md); its **Remember my choice** sets this), **Show it**
  shows the one already here and downloads nothing, **Download again**
  downloads it beside it.
- **Allow the command line to control dm:** "Lets other programs on this
  computer add and stop downloads."
- **Allow AI assistants to control dm (MCP):** "Lets an AI assistant you
  run on this computer add and manage downloads."
- **Check for updates once a week:** dm asks the project's release page
  once a week whether there is a newer dm (see [About dm](about.md)).

## Sounds

- **Play a sound when a download finishes** and **Play a sound when a
  download fails**: the usual sound, or one of your own.
- "A sound file of your own for each, or empty for the usual sound (.wav)."
  There is a field for each of **Download complete**, **Download failed**,
  **Queue started** and **Queue finished**. **Choose…** picks a WAV file;
  **Play** plays the one set.
- **Use the Mica backdrop** (Windows 11 only): "Lets the desktop show
  faintly through the window, as Windows' own apps do." It is used from
  dm's next start.

## Pop-ups

- **Show progress when a download starts:** opens the download's own window
  ([Download progress](download-window.md)) as it starts. "Off: press
  Details on a download's row to open its progress."
- **Show a message when a download finishes:** the
  [Download complete](download-complete.md) window.
- **Offer to download links I copy:** "dm looks at what you copy and offers
  to download any link."
- **Take a copied link at once, without offering it first:** a link you copy
  opens Download details straight away instead of being offered in a bar.
  Greyed while the one above is off.
- **Ask where to save videos:** "Shows Download details before a video from
  a page starts." Off, a video you pick in the browser's **Download video**
  starts without asking.
- **Ask where to save browser downloads:** "Shows Download details before a
  download from your browser starts." Off, a download the browser hands over
  starts at once.
- **Start downloading while Download details is open:** "The file is fetched
  while you choose where it goes; Start now puts it there, Cancel removes
  it."
- **Offer a new link when a download’s link has expired:** "Asks you to
  click the file again on its page, to get a link that works." See
  [Update download link](update-link.md).

## Adding and keeping files

- **Keep the file’s date from the server:** "A saved file gets the date the
  website gives it, not today's."

## The list

**Double-clicking a download** in the list (on the phone, **Tapping a download**): **shows its properties**,
**opens the file**, or **opens its folder**.

## Settings file

- **Save settings to a file…** writes dm's settings to a file you choose
  (`dm-settings.json`), to carry to another computer. It says "Saved to …,
  without any password.": passwords are never written to it.
- **Load settings from a file…** reads such a file into this window. It
  says "Loaded. Look them over, then OK to use them.": nothing is used until
  you click OK.
- If the file cannot be written or read, dm says "… could not be written:
  …" or "… couldn't be read: …" with the reason.
