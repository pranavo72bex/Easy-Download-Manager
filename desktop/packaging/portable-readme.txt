dm, portable

Unzip this folder anywhere (a USB stick is fine) and double-click dm-desktop.exe.

The file named "portable" beside dm-desktop.exe is what makes it portable: dm
keeps its settings and its list of downloads in the "dm-data" folder next to
it, and writes nothing else to the computer. Delete the folder and dm is gone.
A portable dm does not start when you sign in to Windows.

Downloads go to your Downloads folder unless you choose another in Options.

To let the browser extension hand downloads to this copy, open a Command Prompt
in this folder and run:

    dm-desktop.exe --register

Run it again if you move the folder.

dm checks for a newer version and says when one is out; it never installs
anything by itself. The installer, with a Start menu entry and the browser
connection done for you, is on the same Releases page.

https://github.com/pranavo72bex/Easy-Download-Manager
