//! The browser that started the native host, by its process's program name.
//!
//! The name alone is read (`ps` on macOS and Linux, the process list on
//! Windows), and said only where it is a browser dm knows: anything else is
//! nothing to name.

/// A program's name in the words a person knows the browser by: `None` for
/// anything that is no browser dm knows. Case and a Windows `.exe` aside.
pub fn browser_named(program: &str) -> Option<&'static str> {
    let name = program
        .trim()
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .trim();
    let lower = name.to_ascii_lowercase();
    let lower = lower.strip_suffix(".exe").unwrap_or(&lower);
    Some(match lower {
        "microsoft edge" | "msedge" | "microsoft-edge" => "Microsoft Edge",
        "google chrome" | "chrome" | "google-chrome" => "Google Chrome",
        "brave browser" | "brave" | "brave-browser" => "Brave",
        "firefox" | "firefox-bin" => "Firefox",
        "arc" => "Arc",
        "vivaldi" | "vivaldi-bin" => "Vivaldi",
        "opera" => "Opera",
        "chromium" | "chromium-browser" => "Chromium",
        _ => return None,
    })
}

/// Programs a browser starts a native host through on Windows (Chrome and
/// Edge go by `cmd /c`): looked past, to the process that started them.
// Used on Windows, and tested everywhere.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
pub fn is_go_between(program: &str) -> bool {
    let lower = program.trim().to_ascii_lowercase();
    matches!(
        lower.strip_suffix(".exe").unwrap_or(&lower),
        "cmd" | "conhost" | "powershell" | "pwsh"
    )
}

/// The browser this process was started by, where it can be told.
pub fn parent_browser() -> Option<String> {
    #[cfg(unix)]
    {
        let parent = std::os::unix::process::parent_id();
        let out = std::process::Command::new("/bin/ps")
            .args(["-o", "comm=", "-p", &parent.to_string()])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        browser_named(&String::from_utf8_lossy(&out.stdout)).map(str::to_string)
    }
    #[cfg(windows)]
    {
        windows::parent_browser()
    }
    #[cfg(not(any(unix, windows)))]
    {
        None
    }
}

#[cfg(windows)]
mod windows {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    /// Every process's id, its parent's id and its program name, as the
    /// system lists them now.
    fn processes() -> Vec<(u32, u32, String)> {
        let mut all = Vec::new();
        // SAFETY: the snapshot handle is checked before use and closed below;
        // the entry is a plain struct of the size the calls are told.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot == INVALID_HANDLE_VALUE || snapshot.is_null() {
                return all;
            }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut more = Process32FirstW(snapshot, &mut entry) != 0;
            while more {
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|c| *c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(entry.szExeFile.get(..len).unwrap_or(&[]));
                all.push((entry.th32ProcessID, entry.th32ParentProcessID, name));
                more = Process32NextW(snapshot, &mut entry) != 0;
            }
            CloseHandle(snapshot);
        }
        all
    }

    pub(super) fn parent_browser() -> Option<String> {
        super::walk_up(std::process::id(), &processes()).map(str::to_string)
    }
}

/// Up from process `me` through `processes` (id, parent id, program): the
/// first browser dm knows, past any `cmd` it was started through, a few
/// steps at most; `None` at anything else.
// Used on Windows, and tested everywhere.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
pub fn walk_up(me: u32, processes: &[(u32, u32, String)]) -> Option<&'static str> {
    let parent_of = |id: u32| processes.iter().find(|(p, _, _)| *p == id);
    let mut at = parent_of(me)?.1;
    for _ in 0..4 {
        let (_, up, program) = parent_of(at)?;
        if let Some(browser) = browser_named(program) {
            return Some(browser);
        }
        if !is_go_between(program) || *up == at {
            return None;
        }
        at = *up;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browsers_are_named_as_people_know_them_on_every_system() {
        assert_eq!(
            browser_named("/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"),
            Some("Microsoft Edge")
        );
        assert_eq!(browser_named("msedge.exe"), Some("Microsoft Edge"));
        assert_eq!(
            browser_named("C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe"),
            Some("Google Chrome")
        );
        assert_eq!(browser_named("BRAVE.EXE"), Some("Brave"));
        assert_eq!(browser_named("firefox\n"), Some("Firefox"));
        assert_eq!(browser_named("cmd.exe"), None);
        assert_eq!(browser_named("python3"), None);
        assert_eq!(browser_named(""), None);
    }

    #[test]
    fn the_browser_is_found_past_the_cmd_it_starts_the_host_through() {
        let list = |rows: &[(u32, u32, &str)]| -> Vec<(u32, u32, String)> {
            rows.iter()
                .map(|(a, b, c)| (*a, *b, c.to_string()))
                .collect()
        };
        // Edge on Windows: msedge.exe › cmd.exe › the host.
        let edge = list(&[
            (10, 1, "msedge.exe"),
            (20, 10, "cmd.exe"),
            (30, 20, "dm.exe"),
        ]);
        assert_eq!(walk_up(30, &edge), Some("Microsoft Edge"));
        // Firefox starts it itself.
        let firefox = list(&[(10, 1, "firefox.exe"), (30, 10, "dm.exe")]);
        assert_eq!(walk_up(30, &firefox), Some("Firefox"));
        // A script or a terminal is no browser; a loop of ids ends.
        let script = list(&[(10, 1, "python.exe"), (30, 10, "dm.exe")]);
        assert_eq!(walk_up(30, &script), None);
        let looped = list(&[(20, 20, "cmd.exe"), (30, 20, "dm.exe")]);
        assert_eq!(walk_up(30, &looped), None);
        assert_eq!(walk_up(99, &edge), None);
    }
}
