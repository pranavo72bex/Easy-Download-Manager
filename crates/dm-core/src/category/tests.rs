// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

/// A `.bin` is not taken for a program by its extension.
#[test]
fn a_bin_file_is_other_unless_its_name_says_it_installs() {
    use super::{classify, Category};
    for name in ["firmware.bin", "bulk001.bin", "disc.BIN", "weights.bin"] {
        assert_eq!(classify(name, None), Category::Other, "{name}");
    }
    // The server's type does not make one a program either.
    assert_eq!(
        classify("router.bin", Some("application/octet-stream")),
        Category::Other
    );
    for name in ["jdk-setup-x64.bin", "Product-Installer.bin", "Setup64.bin"] {
        assert_eq!(classify(name, None), Category::Apps, "{name}");
    }
    // Words that only contain "install" are not an installer.
    for name in ["uninstall-firmware.bin", "reinstall_image.bin"] {
        assert_eq!(classify(name, None), Category::Other, "{name}");
    }
    // A type the server names still places it.
    assert_eq!(classify("stream.bin", Some("video/mp2t")), Category::Videos);
    assert_eq!(classify("song.bin", Some("audio/mpeg")), Category::Audio);
    assert_eq!(classify("tool.exe", None), Category::Apps);
    assert_eq!(classify("tool.run", None), Category::Apps);
}

#[test]
fn a_folder_name_is_judged_as_it_is_used() {
    assert!(!super::one_folder(" .. "));
    assert!(!super::one_folder(" . "));
    assert!(super::one_folder(" Books "));
}

/// A new name Windows cannot make is refused on every system.
#[test]
fn a_folder_name_windows_cannot_make_is_refused_everywhere() {
    for bad in [
        "Docs: work",
        "CON",
        "nul.txt",
        "COM1",
        "a?",
        "x*y",
        "<a>",
        "pipe|",
        "q\"",
        "end.",
        "tab\there",
    ] {
        assert!(super::portable_folder(bad).is_err(), "{bad:?} was taken");
        // A saved name that works here is still used here.
        assert_eq!(super::one_folder(bad), !cfg!(windows), "{bad:?} here");
    }
    assert!(super::portable_folder("Console games").is_ok());
    assert!(super::one_folder("Console games"));
    assert!(super::portable_folder(" .. ").is_err());
    let mut categories = super::Categories {
        enabled: true,
        ..super::Categories::default()
    };
    categories.folders[0] = " Films ".into();
    assert_eq!(
        categories.destination(std::path::Path::new("/d"), super::Category::ALL[0]),
        std::path::Path::new("/d/Films")
    );
}

use super::*;

fn custom(name: &str, extensions: &[&str], sites: &[&str]) -> Custom {
    Custom {
        name: name.into(),
        extensions: extensions.iter().map(|s| s.to_string()).collect(),
        sites: sites.iter().map(|s| s.to_string()).collect(),
    }
}

#[test]
fn a_category_a_person_made_takes_its_types_from_its_sites() {
    let lectures = custom("Lectures", &["mp4", ".PDF"], &["university.edu"]);
    assert!(lectures.takes("week1.mp4", "university.edu"));
    assert!(
        lectures.takes("notes.pdf", "media.university.edu"),
        "a subdomain"
    );
    assert!(
        !lectures.takes("week1.mp4", "notuniversity.edu"),
        "not a suffix match"
    );
    assert!(
        !lectures.takes("week1.mkv", "university.edu"),
        "another type"
    );

    let anything_from = custom("Work", &[], &["*.corp.example"]);
    assert!(anything_from.takes("q3.xlsx", "files.corp.example"));
    assert!(!anything_from.takes("q3.xlsx", "example.com"));

    let any_site = custom("Fonts", &["ttf", "otf"], &[]);
    assert!(any_site.takes("Inter.ttf", "anywhere.example"));

    assert!(
        !custom("Everything", &[], &[]).takes("a.zip", "x.com"),
        "takes nothing"
    );
}

#[test]
fn a_made_category_comes_before_the_built_in_kinds_and_stays_one_folder() {
    let root = Path::new("/d");
    let mut categories = Categories {
        enabled: true,
        custom: vec![
            custom("../escape", &["zip"], &[]),
            custom("Fonts", &["ttf"], &[]),
            custom("Lectures", &["mp4"], &["university.edu"]),
        ],
        site: "university.edu".into(),
        ..Categories::default()
    };
    assert_eq!(
        categories.place(root, Category::Videos, "w.mp4"),
        root.join("Lectures")
    );
    assert_eq!(
        categories.place(root, Category::Other, "Inter.ttf"),
        root.join("Fonts")
    );
    assert_eq!(
        categories.place(root, Category::Archives, "a.zip"),
        root.join("Archives"),
        "a name that leaves the folder is skipped, not followed"
    );
    categories.site = "youtube.com".into();
    assert_eq!(
        categories.place(root, Category::Videos, "w.mp4"),
        root.join("Videos")
    );
    categories.enabled = false;
    assert_eq!(categories.place(root, Category::Other, "Inter.ttf"), root);
    assert_eq!(
        host_of("https://Media.University.EDU:8443/x.mp4"),
        "media.university.edu"
    );
    assert_eq!(host_of("magnet:?xt=urn:btih:abc"), "");
}

#[test]
fn the_common_kinds_land_where_they_should() {
    for (name, expected) in [
        ("Sintel.2010.1080p.mkv", Category::Videos),
        ("song.MP3", Category::Audio),
        ("report.pdf", Category::Documents),
        ("ubuntu-24.04.iso", Category::Archives),
        ("installer.dmg", Category::Apps),
        ("photo.jpeg", Category::Pictures),
        ("notes", Category::Other),
    ] {
        assert_eq!(classify(name, None), expected, "for {name}");
    }
}

#[test]
fn media_with_no_extension_is_sorted_by_what_the_server_called_it() {
    // Player-served media routinely arrives from a URL with no extension,
    // and those are the downloads people most want sorted.
    assert_eq!(classify("clip", Some("video/mp4")), Category::Videos);
    assert_eq!(
        classify("track", Some("audio/mpeg; charset=x")),
        Category::Audio
    );
    assert_eq!(classify("thing", Some("application/zip")), Category::Other);
}

#[test]
fn the_extension_wins_over_the_content_type() {
    // Servers label things application/octet-stream constantly.
    assert_eq!(
        classify("film.mkv", Some("application/octet-stream")),
        Category::Videos
    );
}

#[test]
fn sorting_off_leaves_everything_in_one_folder() {
    let categories = Categories::default();
    assert!(!categories.enabled);
    assert_eq!(
        categories.destination(Path::new("/d"), Category::Videos),
        Path::new("/d")
    );
}

#[test]
fn sorting_on_nests_by_category() {
    let categories = Categories {
        enabled: true,
        ..Default::default()
    };
    assert_eq!(
        categories.destination(Path::new("/d"), Category::Videos),
        Path::new("/d/Videos")
    );
}

#[test]
fn an_empty_folder_name_means_do_not_nest_that_one() {
    let mut categories = Categories {
        enabled: true,
        ..Default::default()
    };
    categories.folders[Category::ALL
        .iter()
        .position(|c| *c == Category::Other)
        .unwrap()] = String::new();
    assert_eq!(
        categories.destination(Path::new("/d"), Category::Other),
        Path::new("/d")
    );
    assert_eq!(
        categories.destination(Path::new("/d"), Category::Audio),
        Path::new("/d/Audio")
    );
}

#[test]
fn a_folder_name_cannot_escape_the_download_folder() {
    /* These come from a JSON file anyone can edit, and "../.." would put
     * downloads somewhere nobody asked for. */
    let categories = Categories {
        enabled: true,
        folders: vec!["../../etc".into(); Category::ALL.len()],
        ..Categories::default()
    };
    assert_eq!(
        categories.destination(Path::new("/d"), Category::Videos),
        Path::new("/d")
    );
}
