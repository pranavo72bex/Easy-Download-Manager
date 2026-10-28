# Checks on a real Windows PC

These are the checks CI cannot run. CI already installs dm on Windows Server, runs
every test and drives the real program (`windows-check.yml`, `desktop.yml`). What
it lacks is a real GPU, a real login, sleep, SmartScreen and a person's mouse.

**You need:** a Windows 11 PC (22H2 or later, for check 9) with Edge or Chrome,
and about 45 minutes. A Windows 10 PC can run every check except 9.

**How to report:** for each check, write the number, pass or fail, and for a fail
what you saw (a screenshot helps). Open one GitHub issue titled
"Windows checklist, <date>" with the list.

Before starting, download the latest `dm_<version>_x64-setup.exe` from the GitHub
release.

## Installing

1. **SmartScreen.** Run the installer. SmartScreen says "Windows protected your
   PC" because the installer is not signed yet. Click *More info › Run anyway*.
   *Expect:* the installer opens. Note the exact wording SmartScreen showed.
2. **Install and first window.** Finish the installer. *Expect:* dm opens with a
   welcome that offers "Add dm to your browser", with a button for each browser
   you have, and a ticked box "Start dm with the computer, in the tray".
3. **The window draws.** *Expect:* sharp text in Segoe UI Variable, no black or
   flickering areas, and a title bar in the window's colour. Try each choice in
   Options › General › Look: the title bar follows.
4. **High DPI.** In Settings › Display, set scale to 150%, then 200%. *Expect:* dm
   stays sharp and nothing is cut off. Return to your usual scale.

## Browser

5. **Extension hand-off.** Click your browser's button in the welcome, or install
   the extension from the store. Click a download link, for example a `.zip` on
   any site. *Expect:* dm takes it. The browser's own download list stays empty,
   and the file lands in Downloads, byte-exact (open it).
6. **Drag a link in.** Drag a link from the browser onto dm's list. *Expect:* it
   is queued. Then View › Drop target, and drag a link onto the small floating
   target. *Expect:* it is queued too.
7. **Drag a file out.** Drag a finished row from dm onto the desktop or into an
   Explorer window. *Expect:* the file is copied there.

## Tray, login and sleep

8. **Tray.** Close dm's window. *Expect:* a dm icon in the tray, and downloads
   keep running. A left click shows the window. Right click shows Show dm,
   Speed, Scheduler… and Exit dm.
9. **Mica (Windows 11 only).** Options › General › tick "Mica", OK, then Exit dm
   from the tray and open it again. *Expect:* the desktop's colours show
   faintly through the toolbar and side rail. The list stays solid. **If the
   window is black or transparent, that is a fail:** untick Mica and report it.
10. **Start at login.** With the welcome's box left ticked, restart the PC.
    *Expect:* after logging in, dm is in the tray with no window open. In Task
    Manager › Startup apps, dm is listed and enabled.
11. **Wake for the schedule.** Downloads › Scheduler: on, from a time 5 minutes
    ahead, tick "Wake the computer from sleep to start". Add a large download
    with "Add for later". Put the PC to sleep (Start › Power › Sleep). *Expect:*
    the PC wakes at that time and the download starts. If it does not wake,
    check Control Panel › Power Options › Change plan settings › Advanced ›
    Sleep › Allow wake timers is *Enable*, and report what it was set to.

## Files and safety

12. **Notification.** Let a download finish with dm's window hidden. *Expect:* a
    "Download finished" notification. Clicking it opens the file.
13. **Mark of the Web.** Download a small `.exe` (for example the dm installer
    itself) through dm, then run it from Explorer. *Expect:* SmartScreen or the
    "Open File – Security Warning" dialog appears, as for a browser download.
    In the file's Properties, an "Unblock" box is shown.
14. **Full disk.** Download to a nearly full USB stick (Properties › Save to, or
    Download details › folder). *Expect:* the row pauses and says the disk is
    full, with "Choose another folder…". Choose a folder on C:. *Expect:* it
    finishes there.

## Portable and removal

15. **Portable.** Download `dm_…_x64-portable.zip` from the release, unzip it
    to a folder (a USB stick, say), and exit the installed dm if there is one.
    Double-click `dm-desktop.exe` in the unzipped folder. *Expect:* a `dm-data`
    folder appears next to it, the installed dm's list is not shown, and
    nothing new is under `%LOCALAPPDATA%\dm-engine`. (CI runs
    `scripts/portable-check.py` on the zip headless; this is the same on a real
    desktop with the window.) Then, in a Command Prompt in that folder, run
    `dm-desktop.exe --register` and *expect* the browser extension to hand a
    download to this copy.
16. **Narrator.** Turn on Narrator (Ctrl+Win+Enter) and Tab through the toolbar.
    *Expect:* each button's name is read out. Turn Narrator off.
17. **Uninstall.** Settings › Apps › dm › Uninstall. *Expect:* dm is removed,
    the tray icon goes, and dm no longer starts at login. Your downloaded files
    stay.
