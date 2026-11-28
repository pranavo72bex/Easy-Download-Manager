# EDM on a USB stick (Windows)

How to carry EDM on a USB stick, or use it without installing anything,
step by step.

1. From EDM's download page, download `edm_…_x64-portable.zip`
   (`edm_…_arm64-portable.zip` on an ARM laptop).
2. Unzip it onto the USB stick, or into any folder.
3. Double-click `edm-gpui.exe` in that folder. The empty file named
   `portable` beside it makes EDM keep its settings and its list in a
   `edm-data` folder next to it, and write nothing else to the computer.
4. To let your browser's extension hand downloads to this copy, run
   `edm-gpui.exe --register` once in its folder (and again if you move it).

A portable EDM does not start when you sign in. To remove it, delete its
folder: `edm-data` goes with it.
