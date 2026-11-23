# Options › Folders

Where downloads are saved. See [Options](options.md) for how the window
works.

- **Download folder:** where finished downloads go. **Browse…** picks a
  folder.
- **Folder for unfinished downloads:** where the parts of a download wait
  until it is whole. Empty keeps them in dm's own folder. "A download
  already begun keeps the folder it began in."
- **Sort finished files into folders by kind:** each finished file goes into
  a folder for its kind inside the download folder: Videos, Audio,
  Documents, Apps, Archives, Pictures and Other. Under **Folders by kind**, beside each kind is its
  folder's name, which you can change; an empty name leaves that kind
  unsorted, in the download folder itself. While sorting is off, these are
  greyed and the page says "Turn on sorting to set the kinds' folders and
  your own categories."

## Your own categories

"Your own categories come first. A file of these types, from these sites
(any site when empty), goes into a folder named after the category."

- **Add a category** adds a line: **Category** (its name, which is also its
  folder's name), **Types** (file types, separated by spaces, no dots) and
  **Sites** (separated by spaces; a site covers its subdomains).
- **Delete category** takes the line away.
- A category's name must be able to be a folder's name on any computer:
  leave out `/ \ : * ? " < > |`, names like CON, and a dot or space at the
  end. OK refuses one that cannot be.

A site can also have a folder of its own: see **Sites with their own folder,
speed and headers:** in [Options › Advanced](options-advanced.md).

## Unpack when done

"A finished ZIP, 7z or tar archive of a kind turned on here is unpacked into a
new folder of its name beside it. No file is replaced, and nothing is
written outside that folder."

- Under the heading **Unpack when done**, each kind (Videos, Audio,
  Documents, Apps, Archives, Pictures, Other) has a switch. A ZIP, 7z or tar
  file is sorted as **Archives**, so that is the one to turn on. All are off
  in a new install. A single download can be unpacked without its kind:
  tick **Unpack when done** in [Download details](download-details.md).
- The archive is unpacked as **Extract here** unpacks it: dm checks every
  file in it first, and if one would land outside the new folder, or is a
  link, nothing is unpacked. Where a folder of that name is there already,
  the new one is numbered ("pack (1)").
- **Once it is unpacked:** **Keep the archive** (the choice in a new
  install) leaves it where it was; **Send the archive to the Trash** moves
  it to the Recycle Bin (the Trash on macOS) once its files are out, so it
  can be got back. An archive that was not unpacked is always kept.
- **Archive passwords:** "Passwords tried on an archive that has one, in
  turn; the one that opens it goes first." Type one in **Add a password**
  and click **Add**; each kept is listed as dots (never the password, nor
  how long it is), with **Delete**. They are kept when you click **OK**,
  sealed apart from the settings as the sign-ins are, and never shown,
  logged or put in a backup. "An archive none of these passwords opens is
  left as it is, and its row says so."
- What became of it is said on the download's row, after "Complete": see
  [Notes beside a download's state](failures.md#notes-beside-a-downloads-state).
