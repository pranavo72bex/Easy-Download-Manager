// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

fn links(urls: &[&str]) -> LinksImport {
    LinksImport {
        items: urls.iter().map(|u| (u.to_string(), true)).collect(),
        hide_html: true,
        hide_images: true,
        hide_repeats: true,
        use_texts: true,
        queue: dm_core::MAIN_QUEUE.into(),
        ..LinksImport::default()
    }
}

#[test]
fn pages_pictures_and_repeats_are_left_out_and_types_filter() {
    let mut d = links(&[
        "https://e.com/a.zip",
        "https://e.com/page.html",
        "https://e.com/pic.png",
        "https://e.com/a.zip",
        "https://e.com/b.pdf",
    ]);
    assert_eq!(d.visible(), vec![true, false, false, false, true]);
    d.types = "pdf".into();
    assert_eq!(d.chosen(), vec!["https://e.com/b.pdf".to_string()]);
}

#[test]
fn what_is_added_carries_the_page_and_says_how_many() {
    let mut d = links(&["https://e.com/a.zip"]);
    d.page = "https://e.com/".into();
    d.texts
        .insert("https://e.com/a.zip".into(), "The archive".into());
    let root = std::path::Path::new("/tmp");
    let (items, _) = d.adds(true, root, &dm_core::Categories::default()).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].1.referrer, "https://e.com/");
    assert_eq!(items[0].1.description, "The archive");
    assert_eq!(items[0].1.queue, dm_core::MAIN_QUEUE);
    d.save.save_to = SaveTo::OneFolder;
    assert_eq!(
        d.adds(false, root, &dm_core::Categories::default())
            .unwrap_err(),
        "Choose the folder every file goes to."
    );
    assert_eq!(added_words(2, false, ""), "Started 2 downloads.");
}

fn import(urls: &[&str]) -> LinksImport {
    LinksImport {
        items: urls.iter().map(|u| (u.to_string(), true)).collect(),
        ..LinksImport::default()
    }
}

#[test]
fn only_these_types_go_by_the_servers_name_or_the_addresses() {
    let mut d = import(&[
        "http://h/a.zip",
        "http://h/b.PDF?x=1",
        "http://h/get?id=3",
        "http://h/c.mp4",
    ]);
    d.types = ".zip, pdf".into();
    // The third is named by its server: a zip.
    dm_core::lock(&d.found).insert(
        "http://h/get?id=3".into(),
        Found {
            name: "named.zip".into(),
            ..Found::default()
        },
    );
    assert_eq!(d.visible(), [true, true, true, false]);
    assert_eq!(
        d.chosen(),
        ["http://h/a.zip", "http://h/b.PDF?x=1", "http://h/get?id=3"]
    );
    d.types = "  ".into();
    assert!(d.visible().iter().all(|v| *v), "nothing typed lists all");
}

#[test]
fn the_folder_every_link_goes_to_is_the_choice_made() {
    use dm_shell::app::names::{category_folder, category_word};
    let root = std::path::Path::new("downloads");
    let sorted = dm_core::Categories {
        enabled: true,
        ..dm_core::Categories::default()
    };
    let mut d = import(&["http://h/a.zip", "http://h/b.mp4"]);
    assert_eq!(d.save.save_dir(root, &sorted), None, "each to its own");
    assert_eq!(
        d.folder_of("http://h/b.mp4", root, &sorted),
        sorted
            .destination(root, dm_core::Category::Videos)
            .display()
            .to_string()
    );
    d.save.save_to = SaveTo::OneFolder;
    d.save.folder = "   ".into();
    assert_eq!(d.save.save_dir(root, &sorted), None, "no folder typed");
    d.save.folder = " elsewhere ".into();
    assert_eq!(d.save.save_dir(root, &sorted).as_deref(), Some("elsewhere"));
    assert_eq!(d.folder_of("http://h/a.zip", root, &sorted), "elsewhere");
    d.save.save_to = SaveTo::OneCategory;
    d.save.category = category_word(dm_core::Category::Documents);
    assert_eq!(
        d.save.save_dir(root, &sorted),
        Some(
            sorted
                .destination(root, dm_core::Category::Documents)
                .display()
                .to_string()
        )
    );
    // A person's own category is a folder of its name.
    d.save.category = "Receipts".into();
    assert_eq!(
        category_folder(&d.save.category, root, &sorted),
        root.join("Receipts")
    );
}

#[test]
fn a_name_given_with_edit_wins_over_the_servers() {
    let mut d = import(&["http://h/a.zip"]);
    dm_core::lock(&d.found).insert(
        "http://h/a.zip".into(),
        Found {
            name: "server.zip".into(),
            ..Found::default()
        },
    );
    assert_eq!(d.name_of("http://h/a.zip"), "server.zip");
    d.names.insert("http://h/a.zip".into(), "  ".into());
    assert_eq!(d.name_of("http://h/a.zip"), "server.zip", "blank is none");
    d.names.insert("http://h/a.zip".into(), " mine.zip ".into());
    assert_eq!(d.name_of("http://h/a.zip"), "mine.zip");
}
