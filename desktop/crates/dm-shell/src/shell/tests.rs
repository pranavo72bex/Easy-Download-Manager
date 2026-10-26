/// An uninstall asks a running dm to end, and leaves nothing
/// when none runs, or the next launch would end on its own.
#[test]
fn a_quit_is_asked_only_of_a_running_dm() {
    let home = tempfile::tempdir().unwrap();
    assert!(!ask_running_to_quit(home.path()).unwrap());
    assert!(!host::inbox(home.path()).join(QUIT).exists());
    let _held = crate::instance::claim(home.path())
        .unwrap()
        .expect("the lock");
    assert!(ask_running_to_quit(home.path()).unwrap());
    assert!(host::inbox(home.path()).join(QUIT).exists());
}

#[test]
fn a_launch_that_hands_off_registers_first_and_still_hands_off_when_that_fails() {
    let home = tempfile::tempdir().unwrap();
    let show = crate::host::inbox(home.path()).join("show");
    let mut ran = false;
    let failed = super::hand_off(home.path(), &super::Orders::default(), || {
        ran = true;
        assert!(!show.exists(), "handed off before registering");
        Ok(None)
    });
    assert!(ran && failed.is_none() && show.exists());
    std::fs::remove_file(&show).unwrap();
    let failed = super::hand_off(home.path(), &super::Orders::default(), || Err("no".into()));
    assert_eq!(failed.as_deref(), Some("no"));
    assert!(show.exists(), "a failed registration stopped the hand-off");
}

#[test]
fn a_relative_save_folder_is_the_callers_and_never_kept_relative() {
    let args: Vec<String> = ["--save-to", "myout", "https://files.example/a.zip"]
        .map(String::from)
        .to_vec();
    let orders = super::orders(&args);
    let here = std::env::current_dir().unwrap();
    assert_eq!(
        orders.downloads[0].save_dir.as_deref(),
        Some(here.join("myout").display().to_string().as_str())
    );
}

#[tokio::test]
async fn a_pasted_list_skips_the_link_it_cannot_use_and_adds_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let settings = crate::settings::Settings {
        download_dir: dir.path().join("out"),
        ..Default::default()
    };
    let queue = dm_core::Queue::new(
        dm_core::client(),
        settings.options(dir.path().join("parts"), dm_core::Limiter::unlimited()),
        1,
    );
    let added = super::add(
        &queue,
        "http:// http://127.0.0.1:9/a.bin http://127.0.0.1:9/b.bin",
    )
    .await
    .expect("one bad link refused the whole paste");
    assert_eq!(added.ids.len(), 2);
    assert_eq!(added.skipped.len(), 1, "{:?}", added.skipped);
    assert!(added.skipped[0].contains("names no site"));
    assert!(super::add(&queue, "http://").await.is_err());
}

#[test]
fn an_address_with_no_site_is_refused_with_a_plain_reason() {
    assert!(super::unusable("http://")
        .unwrap()
        .contains("names no site"));
    assert_eq!(super::unusable("https://files.example/a.zip"), None);
}

#[test]
fn a_link_with_no_site_as_typed_is_refused_though_it_parses() {
    // The parser reads `http:///x` as a site called `x`.
    let password = crate::test_secrets::secret("password");
    let no_site = format!("http://user:{password}@/x");
    let signed_in = format!("http://user:{password}@e.com/x");
    for typed in [
        "http:///x",
        "https:///files/a.zip",
        "ftp:///pub/a",
        no_site.as_str(),
        "http://:8080/x",
        "http://?q",
    ] {
        let why = super::unusable(typed).unwrap_or_default();
        assert!(why.contains("names no site"), "{typed}: {why:?}");
    }
    for fine in [
        signed_in.as_str(),
        "HTTP://E.COM/A",
        "http://127.0.0.1:9/a",
        "http://[::1]:9/a",
    ] {
        assert_eq!(super::unusable(fine), None, "{fine}");
    }
}

#[test]
fn a_magnet_without_a_torrent_hash_is_refused_at_add() {
    for bad in [
        "magnet:?xt=urn:btih:zz",
        "magnet:?xt=urn:btih:abc",
        "magnet:?dn=film",
        "magnet:",
        "magnet:?xt=urn:sha1:0123456789abcdef0123456789abcdef01234567",
        "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef0123456g",
    ] {
        let why = super::unusable(bad).unwrap_or_default();
        assert!(why.contains("names no torrent"), "{bad}: {why:?}");
    }
    for good in [
        "magnet:?xt=urn:btih:0123456789ABCDEF0123456789abcdef01234567&dn=x",
        "magnet:?dn=x&xt=urn:btih:ABCDEFGHIJKLMNOPQRSTUVWXYZ234567",
        "magnet:?xt=urn:btmh:12200123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    ] {
        assert_eq!(super::unusable(good), None, "{good}");
    }
}

#[test]
fn a_command_line_is_split_as_a_shell_splits_it() {
    assert_eq!(super::words("clamscan --quiet"), ["clamscan", "--quiet"]);
    assert_eq!(
        super::words(r#""/Applications/My Scanner.app/x" -a 'b c' d\ e"#),
        ["/Applications/My Scanner.app/x", "-a", "b c", "d e"]
    );
    assert!(super::words("   ").is_empty());
    assert_eq!(super::words(r#"x """#), ["x", ""]);
}

#[test]
fn open_with_names_the_bundle_on_macos_and_runs_the_program_elsewhere() {
    let file = Path::new("/d/file.zip");
    let (program, args) = open_with_command(file, Path::new("/Applications/Unarchiver.app"), true);
    assert_eq!(program, PathBuf::from("open"));
    assert_eq!(args, ["-a", "/Applications/Unarchiver.app", "/d/file.zip"]);
    let (program, args) = open_with_command(file, Path::new("/usr/bin/file-roller"), false);
    assert_eq!(program, PathBuf::from("/usr/bin/file-roller"));
    assert_eq!(args, ["/d/file.zip"]);
    assert!(open_with("/no/such/file", Path::new("/bin/sh")).is_err());
}

use super::*;

/// No `xdg-utils`, as on a minimal desktop: GNOME's `gio` opens it.
#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn a_desktop_without_xdg_open_opens_with_its_own_opener() {
    use std::os::unix::fs::PermissionsExt;
    let bin = tempfile::tempdir().unwrap();
    let said = bin.path().join("said");
    let gio = bin.path().join("gio");
    std::fs::write(
        &gio,
        format!("#!/bin/sh\necho \"$@\" > '{}'\n", said.display()),
    )
    .unwrap();
    std::fs::set_permissions(&gio, std::fs::Permissions::from_mode(0o755)).unwrap();
    let file = bin.path().join("f.txt");
    linux_open_in(&file, Some(bin.path().as_os_str())).unwrap();
    for _ in 0..100 {
        if said.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let said = std::fs::read_to_string(&said).unwrap();
    assert_eq!(said.trim(), format!("open {}", file.display()));

    // With no opener at all, the fix is named.
    let empty = tempfile::tempdir().unwrap();
    let refused = linux_open_in(&file, Some(empty.path().as_os_str())).unwrap_err();
    assert!(refused.contains("xdg-utils"), "{refused}");
}

/// `xdg-open` that runs and fails ("no method available", exit 3) was
/// taken as opened: the next opener is tried, and when it
/// fails too, what each said is the error.
#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn an_opener_that_runs_and_fails_passes_to_the_next_and_says_why() {
    use std::os::unix::fs::PermissionsExt;
    let script = |dir: &Path, name: &str, body: &str| {
        let p = dir.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    };
    let bin = tempfile::tempdir().unwrap();
    let file = bin.path().join("f.txt");
    let said = bin.path().join("said");
    script(
        bin.path(),
        "xdg-open",
        "echo 'xdg-open: no method available for opening' >&2; exit 3",
    );
    script(
        bin.path(),
        "gio",
        &format!("echo \"$@\" > '{}'", said.display()),
    );
    linux_open_in(&file, Some(bin.path().as_os_str())).unwrap();
    let opened = std::fs::read_to_string(&said).expect("gio was never tried");
    assert_eq!(opened.trim(), format!("open {}", file.display()));

    // Every opener there fails: the error is theirs, not silence.
    script(bin.path(), "gio", "echo 'gio: no handler' >&2; exit 2");
    let refused = linux_open_in(&file, Some(bin.path().as_os_str())).unwrap_err();
    assert!(refused.contains("no method available"), "{refused}");
    assert!(refused.contains("gio: no handler"), "{refused}");

    // An opener still running at the deadline is opening the file.
    let slow = tempfile::tempdir().unwrap();
    script(slow.path(), "xdg-open", "exec /bin/sleep 5");
    let started = std::time::Instant::now();
    linux_open_in(&file, Some(slow.path().as_os_str())).unwrap();
    assert!(started.elapsed() < std::time::Duration::from_secs(3));
}

/* One binary is both the window and the host, and which one it is comes
 * down to reading the arguments a browser passes. Firefox passes the
 * manifest path and nothing else; getting that wrong opened a window,
 * wrote nothing to stdout, and left the browser holding a port that
 * never answered — with no error anywhere. */
#[test]
fn each_browser_is_recognised_by_what_it_actually_passes() {
    let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();

    // Firefox: the path of the app manifest, and that is all.
    assert!(browser_launch(&args(&[
        "/Users/x/Library/Application Support/Mozilla/NativeMessagingHosts/com.dm.engine.json"
    ])));
    // Linux puts it somewhere else entirely.
    assert!(browser_launch(&args(&[
        "/home/x/.mozilla/native-messaging-hosts/com.dm.engine.json"
    ])));
    // Chrome: the calling extension's origin.
    assert!(browser_launch(&args(&[
        "chrome-extension://abcdefghijklmnop/",
        "--parent-window=0"
    ])));

    // And a person, who passes nothing, or opens a file.
    assert!(!browser_launch(&args(&[])));
    assert!(!browser_launch(&args(&["--help"])));
    assert!(!browser_launch(&args(&["--desktop"])));
    assert!(!browser_launch(&args(&["/Users/x/notes.json"])));
}

/* A link or a name that merely ends like the manifest, or a link to an
 * extension's page, is a person's download: it used to make the launch
 * a host that read nothing and dropped the link. */
#[test]
fn a_link_or_a_name_never_makes_a_launch_the_browsers() {
    let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    // Firefox's own shape, with the extension's id after it.
    assert!(browser_launch(&args(&[
        "/home/x/.mozilla/native-messaging-hosts/com.dm.engine.json",
        "dm@example.org"
    ])));
    assert!(browser_launch(&args(&[
        "chrome-extension://abcdefghijklmnop/"
    ])));
    assert!(browser_launch(&args(&["--native-host"])));

    for person in [
        &["--later", "https://127.0.0.1:9/dir/com.dm.engine.json"][..],
        &["https://127.0.0.1:9/dir/com.dm.engine.json"],
        &[
            "--later",
            "--name",
            "com.dm.engine.json",
            "https://e.com/b.zip",
        ],
        &["--name", "/tmp/com.dm.engine.json", "https://e.com/b.zip"],
        &[
            "https://e.com/a.zip",
            "chrome-extension://abcdefghijklmnop/",
        ],
        &[
            "chrome-extension://abcdefghijklmnop/",
            "https://e.com/a.zip",
        ],
        &["/home/x/com.dm.engine.json", "https://e.com/a.zip"],
        &["/home/x/com.dm.engine.json", "--later"],
        &["com.dm.engine.json"],
        &["https://e.com/a.zip", "--native-host"],
    ] {
        assert!(!browser_launch(&args(person)), "{person:?}");
    }
}

fn argv(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn only_links_are_taken_from_the_command_line() {
    let args = argv(&["--hidden", "https://e.com/a", "notes.txt", "magnet:?xt=x"]);
    let urls: Vec<String> = orders(&args).downloads.into_iter().map(|r| r.url).collect();
    assert_eq!(urls, ["https://e.com/a", "magnet:?xt=x"]);
    assert!(launched_at_login(&args));
}

#[test]
fn a_mistyped_command_is_found_and_real_arguments_are_not() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("notes.txt");
    std::fs::write(&file, b"x").unwrap();
    let file = file.display().to_string();
    let args = argv(&[
        "--control",
        "0",
        "--save-to",
        "somewhere",
        "--name",
        "a.bin",
        "--when-done",
        "exit",
        "--hidden",
        "-psn_0_1",
        "https://e.com/a",
        "magnet:?xt=x",
        "C:\\x\\y.torrent",
        &file,
    ]);
    assert!(unrecognised(&args).is_empty(), "{:?}", unrecognised(&args));
    assert_eq!(
        unrecognised(&argv(&["lst", "--later", "fly"])),
        ["lst", "fly"]
    );
}

#[test]
fn a_torrent_on_this_computer_is_taken_by_path_or_file_address() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a b.torrent");
    std::fs::write(&file, b"d4:infod4:name1:xee").unwrap();
    let address = dm_core::torrent::file_url(&file).unwrap();
    let other = dir.path().join("notes.txt");
    std::fs::write(&other, b"x").unwrap();
    let got: Vec<String> = orders(&argv(&[
        file.to_str().unwrap(),
        address.as_str(),
        other.to_str().unwrap(),
        dir.path().join("gone.torrent").to_str().unwrap(),
    ]))
    .downloads
    .into_iter()
    .map(|r| r.url)
    .collect();
    assert_eq!(got, [address.clone(), address]);
}

#[test]
fn when_done_is_read_and_left_for_the_window() {
    let got = orders(&argv(&["--start-queue", "--when-done", "Shutdown"]));
    assert_eq!(got.when_done.as_deref(), Some("shutdown"));
    assert!(orders(&argv(&["--when-done", "explode"]))
        .when_done
        .is_none());
    let home = tempfile::tempdir().unwrap();
    hand_to_running(home.path(), &got);
    let left = std::fs::read_to_string(host::inbox(home.path()).join(WHEN_DONE_FILE));
    assert_eq!(left.unwrap(), "shutdown");
}

#[test]
fn new_download_is_read_and_left_for_the_window_that_opens_add_url() {
    let got = orders(&argv(&["--new-download"]));
    assert!(got.new_download);
    assert!(got.downloads.is_empty(), "it is no link");
    assert!(!orders(&argv(&[])).new_download);
    assert!(
        unrecognised(&argv(&["--new-download"])).is_empty(),
        "an option, not a mistyped command"
    );
    let home = tempfile::tempdir().unwrap();
    hand_to_running(home.path(), &got);
    assert!(host::inbox(home.path()).join(NEW_DOWNLOAD_FILE).exists());
    let other = tempfile::tempdir().unwrap();
    hand_to_running(other.path(), &orders(&argv(&["--start-queue"])));
    assert!(!host::inbox(other.path()).join(NEW_DOWNLOAD_FILE).exists());
}

#[test]
fn options_apply_to_the_link_after_them() {
    // A folder that is whole on this system: on Windows "/tmp/in" is
    // taken as on the current drive, "D:/tmp/in" (CI, 0.2.0).
    let folder = std::env::temp_dir().join("in").display().to_string();
    let got = orders(&argv(&[
        "--save-to",
        folder.as_str(),
        "--name",
        "b.zip",
        "--later",
        "https://e.com/a.zip",
        "https://e.com/c.iso",
        "--start-queue",
        "/data",
    ]));
    assert_eq!(got.downloads.len(), 2);
    let first = &got.downloads[0];
    assert_eq!(first.url, "https://e.com/a.zip");
    assert_eq!(first.save_dir.as_deref(), Some(folder.as_str()));
    assert_eq!(first.save_name.as_deref(), Some("b.zip"));
    assert!(first.later);
    let second = &got.downloads[1];
    assert_eq!(
        (second.save_dir.as_deref(), second.later),
        (None, false),
        "not carried over"
    );
    assert!(got.start_queue);
}

#[test]
fn options_with_no_link_after_them_do_nothing() {
    assert_eq!(
        orders(&argv(&["--name", "x", "--later"])),
        Orders::default()
    );
    assert_eq!(
        orders(&argv(&["--save-to"])),
        Orders::default(),
        "a flag at the end"
    );
    assert_eq!(orders(&argv(&["notes.txt"])), Orders::default());
}

#[test]
fn an_empty_checksum_clears_and_nonsense_is_refused() {
    assert_eq!(checksum("  ").unwrap(), None);
    assert!(checksum("not a digest").is_err());
    assert!(checksum(&"a".repeat(64)).unwrap().is_some());
}
