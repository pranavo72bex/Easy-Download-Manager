# Options

Options holds dm's settings. On macOS open it from **dm › Settings…** or ⌘,
(comma), as every Mac program's settings; on Windows and Linux from
**Downloads › Options**, the toolbar's **Options** button, or Ctrl+, (comma). It has six pages, listed at
its left (on a phone, a list of their own):

- [General](options-general.md): language, looks, starting, sounds, pop-ups
  and the list.
- [Browser](options-browser.md): which downloads dm takes from your browser,
  and who opens magnet links and `.torrent` files.
- [Folders](options-folders.md): where downloads are saved, and sorting
  them by kind.
- [Downloads](options-downloads.md): connections, how many files at once,
  video quality, queues and what happens when a download finishes.
- [Advanced](options-advanced.md): the network (speed limit, attempts,
  time-outs), sites with rules of their own, the proxy, Dial Up / VPN and
  BitTorrent.
- [Sign-ins](options-sign-ins.md): a user and password for a site.

## Finding a setting

The box left of the tabs, **Find a setting**, searches every page as you
type: each setting whose words hold what you typed is listed under the name
of its page. Click the page's name to open that page. When nothing matches,
it says "No setting says “…”."

## Changing settings

- A check box, a choice or a button changes the page at once; a field
  changes as you type. Nothing is used until you click **OK**.
- **OK** keeps only what you changed in Options. A setting changed elsewhere
  while Options was open (in a menu, say) keeps that change.
- **Cancel** closes Options and keeps nothing.
- **Restore defaults** puts the settings of the page you are on back to how
  dm first had them. The Sign-ins page has no defaults to go back to, so it
  has no such button.
- A field that takes a number keeps the number it had if you type something
  else, and says "Numbers only: the one before was kept" under it.
- **Text size** on the General page is used at once, and kept whatever you
  click.

OK refuses, and says why on the page that holds the problem, when:

- the proxy is turned on with no host ("Enter the proxy's host, or untick
  "Connect through a proxy"."), with a host that is an address rather than a
  name, or with no port. See [Proxies](proxies.md).
- a category you made or renamed cannot be a folder's name. Leave out
  `/ \ : * ? " < > |`, names like CON, and a dot or space at the end.

While a program was driving dm's window a moment before (the command line or
an AI assistant, see **Options › General**), OK does not change the command
run when a download finishes, the program run when the scheduled downloads
are done, the proxy script, or the download folders: those are yours to set.
dm says which one was kept and asks you to set it again.
