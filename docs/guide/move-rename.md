# Move/Rename

Move/Rename moves finished files to another folder, and renames them. Open
it with **File › Move/Rename…** or **Move/Rename** on the right-click menu.
It is greyed unless every chosen download has finished and its file is
still where dm saved it, or one download is chosen that failed or is
stopped (below).

## One file

- **New name:** the file's name. The name before its extension is
  selected, so you can type over it and keep the extension.
- **Folder:** the folder the file is in. Change it, or click **Browse…** to
  choose one in the system's folder panel, to move the file there.

## A download that failed or is stopped

A download that failed because something of its name is already there
has **Move/Rename** beside **Retry** in its window. It, and any one
download that failed or is stopped (at a folder dm cannot write to, say),
has it on the right-click menu too. **New name** is the
name its file will get, and **Folder** where it goes; what it has fetched
goes with it. **OK** tries it again at once. A download you stopped
yourself keeps its new name and folder and stays stopped.

## Several files

- The window says "Moves … finished files."
- **Name pattern:** leave it empty to keep the files' names. Otherwise each
  file is renamed by the pattern: `{n}` is the file's number, counting
  from 1, and `{name}` is its old name without its extension. Each file
  keeps its extension.
- **Folder** and **Browse…:** the folder they all move to.

## Buttons

- **Cancel:** closes the window and changes nothing.
- **OK:** moves and renames the files. If a name is taken in that folder,
  the file gets a free one and the status bar says "… is taken in that
  folder, so it is called …." Anything that could not be done is said in
  the status bar.

## Keys

- **Return** in any field is **OK**.
- **Escape**, Ctrl+W (⌘W on macOS) and the close button are **Cancel**.
