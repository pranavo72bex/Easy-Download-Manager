use super::*;

/// The language in force for one test, put back to English after it.
fn in_language<T>(code: &str, run: impl FnOnce() -> T) -> T {
    set(code);
    let out = run();
    set("en");
    out
}

fn pieces_of(template: &str) -> Vec<Piece<'_>> {
    pieces(template).collect()
}

#[test]
fn a_built_in_queue_typed_in_the_language_shown_is_that_queue() {
    let shown = in_language("es", || name(dm_core::MAIN_QUEUE));
    assert_eq!(shown, "Cola principal");
    let main = in_language("es", || queue_named(" Cola principal "));
    assert_eq!(main, dm_core::MAIN_QUEUE);
    assert_eq!(queue_named(dm_core::MAIN_QUEUE), dm_core::MAIN_QUEUE);
    assert_eq!(queue_named(" Nightly "), "Nightly");
}

/* ---------------------------------------------------------- t */

#[test]
fn english_reads_as_written_and_needs_no_catalog() {
    in_language("en", || {
        assert_eq!(t("Cancel"), "Cancel");
        assert_eq!(t(""), "");
    });
}

#[test]
fn a_language_reads_its_catalog() {
    in_language("de", || {
        assert_eq!(t("Cancel"), "Abbrechen");
        assert_eq!(t("Name"), "Name");
        assert_eq!(t("Speed"), "Geschwindigkeit");
    });
    in_language("ru", || assert_eq!(t("Cancel"), "Отмена"));
}

#[test]
fn an_unknown_language_is_english() {
    for code in ["xx", "", "DE", "de-DE", "en-US"] {
        in_language(code, || assert_eq!(t("Cancel"), "Cancel", "{code:?}"));
    }
}

#[test]
fn a_word_no_catalog_has_reads_in_english() {
    in_language("fr", || {
        assert_eq!(t("Nothing in any catalog"), "Nothing in any catalog");
    });
}

#[test]
fn the_language_is_this_threads_alone() {
    set("de");
    let elsewhere = std::thread::spawn(|| t("Cancel")).join().unwrap();
    assert_eq!(elsewhere, "Cancel", "another thread is still in English");
    assert_eq!(t("Cancel"), "Abbrechen");
    set("en");
}

/* ------------------------------------------------ missing words */

#[test]
fn what_has_no_translation_is_recorded_only_when_asked() {
    in_language("es", || {
        let _ = t("Not recorded: nothing is listening");
        assert!(take_missing().is_empty(), "recorded without being asked");

        record_missing();
        let _ = t("Cancel");
        let _ = t("Missing one");
        let _ = t("Missing one");
        let _ = t("Missing two");
        assert_eq!(take_missing(), ["Missing one", "Missing two"]);
        assert!(take_missing().is_empty(), "taking stops the recording");
    });
}

#[test]
fn english_records_nothing_missing() {
    in_language("en", || {
        record_missing();
        let _ = t("Anything at all");
        assert!(take_missing().is_empty());
    });
}

/* ---------------------------------------------- choosing a language */

#[test]
fn a_setting_names_a_language_or_follows_the_system() {
    assert_eq!(chosen("de"), "de");
    assert_eq!(chosen("pt-BR"), "pt-BR");
    for unset in ["", "xx", "DE", "pt"] {
        assert_eq!(chosen(unset), system(), "{unset:?}");
    }
}

#[test]
fn every_language_is_named_in_itself() {
    assert_eq!(name_of("ru"), "Русский");
    assert_eq!(name_of("tr"), "Türkçe");
    assert_eq!(name_of("zz"), "English", "unknown");
    for (code, name) in LANGUAGES {
        assert_eq!(name_of(code), name);
    }
}

#[test]
fn a_locale_comes_to_one_of_dms_languages() {
    let cases = [
        (Some("de_DE.UTF-8"), None, "de"),
        (Some("de_DE.UTF-8@euro"), None, "de"),
        (Some("fr_CA"), None, "fr"),
        (Some("pt_BR.UTF-8"), None, "pt-BR"),
        (Some("pt_PT"), None, "pt-BR"),
        (Some("ru"), None, "ru"),
        (Some("es-419"), None, "es"),
        (Some("TR_tr"), None, "tr"),
        // The C library's own: English, whatever the system says.
        (Some("C"), Some("de-DE"), "en"),
        (Some("C.UTF-8"), Some("de-DE"), "en"),
        (Some("POSIX"), Some("de-DE"), "en"),
        // Nothing in the environment: the system's.
        (None, Some("id-ID"), "id"),
        (Some(""), Some("es-ES"), "es"),
        // Chinese, Japanese and Korean, as macOS and Linux spell them.
        (Some("ja_JP.UTF-8"), None, "ja"),
        (Some("zh-Hans-CN"), None, "zh-CN"),
        (None, Some("ko-KR"), "ko"),
        // Languages dm does not have.
        (Some("ar_EG.UTF-8"), None, "en"),
        (None, None, "en"),
        (Some("@"), None, "en"),
        (Some("."), None, "en"),
    ];
    for (environment, os, wanted) in cases {
        assert_eq!(resolve(environment, os), wanted, "{environment:?} / {os:?}");
    }
}

#[test]
fn the_system_language_is_read_once_and_is_offered() {
    let found = system();
    assert!(offered(found).is_some(), "{found}");
    assert!(std::ptr::eq(found, system()), "the same each time");
}

#[test]
fn the_picker_offers_every_language_and_picks_none_unclicked() {
    let ctx = egui::Context::default();
    let mut picked = Some("unset");
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        picked = pick(ui, "de");
    });
    output.textures_delta.clear();
    assert_eq!(picked, None);
}

/* ------------------------------------------------------- names */

#[test]
fn only_the_names_dm_gives_are_translated() {
    in_language("de", || {
        assert_eq!(name(dm_core::MAIN_QUEUE), "Haupt-Warteschlange");
        assert_eq!(name("Audio"), "Audio");
        // A person's own, even one that is a word the catalog has.
        assert_eq!(name("Finished"), "Finished");
        assert_eq!(name("audio"), "audio", "only as dm spells it");
        assert_eq!(name("My queue"), "My queue");
        assert_eq!(name(""), "");
    });
    in_language("en", || assert_eq!(name("Audio"), "Audio"));
}

/* ------------------------------------------------ placeholders */

#[test]
fn values_fill_their_places_in_turn_or_by_position() {
    assert_eq!(
        fill("{} of {} pieces done", &[&3, &8]),
        "3 of 8 pieces done"
    );
    assert_eq!(fill("{1} then {0}", &[&"a", &"b"]), "b then a");
    assert_eq!(
        fill("{0} and {0}", &[&"x"]),
        "x and x",
        "a value used twice"
    );
    assert_eq!(fill("{}{}", &[&1, &2]), "12", "places side by side");
    assert_eq!(fill("{01}", &[&"a", &"b"]), "b", "leading zero");
    assert_eq!(fill("Größe: {} МБ", &[&"5"]), "Größe: 5 МБ", "not ASCII");
}

#[test]
fn a_template_with_no_places_is_itself() {
    assert_eq!(fill("", &[]), "");
    assert_eq!(fill("plain", &[&1]), "plain", "a spare value is unused");
}

#[test]
fn a_brace_that_is_not_a_place_is_words() {
    for (template, wanted) in [
        ("a {brace} {}", "a {brace} 1"),
        ("{", "{"),
        ("}", "}"),
        ("{{}", "{1"),
        ("ends with {", "ends with {"),
        ("{ }", "{ }"),
        ("{+1}", "{+1}"),
        ("{-1}", "{-1}"),
        ("{99999999999999999999999}", "{99999999999999999999999}"),
    ] {
        assert_eq!(fill(template, &[&1]), wanted, "{template:?}");
    }
}

#[test]
fn a_place_with_no_value_is_kept_as_written() {
    assert_eq!(fill("{} and {}", &[&"a"]), "a and {}");
    assert_eq!(fill("{3}", &[&"a"]), "{3}");
    assert_eq!(fill("{}", &[]), "{}");
}

#[test]
fn a_value_is_never_read_for_places_of_its_own() {
    assert_eq!(fill("Found {}.", &[&"{} {0}"]), "Found {} {0}.");
}

#[test]
fn a_template_is_cut_into_words_and_places() {
    assert_eq!(
        pieces_of("a {} b {1}"),
        [
            Piece::Words("a "),
            Piece::Value(0, "{}"),
            Piece::Words(" b "),
            Piece::Value(1, "{1}"),
        ]
    );
    assert_eq!(pieces_of(""), []);
    assert_eq!(pieces_of("{}"), [Piece::Value(0, "{}")]);
    assert_eq!(
        pieces_of("x {y} {}"),
        [Piece::Words("x {y} "), Piece::Value(0, "{}")]
    );
}

#[test]
fn the_values_a_template_takes_are_listed_by_position() {
    assert_eq!(placeholders("{1} then {0}"), placeholders("{} and {}"));
    assert_eq!(placeholders("a {} b"), [0]);
    assert_eq!(placeholders("no places"), Vec::<usize>::new());
    assert_eq!(placeholders("{brace} {}"), [0]);
    assert_eq!(placeholders("{0} {0}"), [0, 0]);
}

#[test]
fn tf_fills_the_translation() {
    in_language("de", || {
        assert_eq!(tf("Version {}", &[&"1.0"]), "Version 1.0");
        // German puts the seconds first: the catalog names its values.
        assert_eq!(
            tf("dm will {} in {} seconds.", &[&"beenden", &30]),
            "dm wird in 30 Sekunden beenden."
        );
    });
    in_language("en", || {
        assert_eq!(tf("Version {}", &[&"1.0"]), "Version 1.0");
    });
}

/* ----------------------------------------------------- Said */

#[test]
fn a_sentence_is_english_for_scripts_and_translated_for_people() {
    let said = Said::counting("Imported {} download.||Imported {} downloads.", 3, &[&3]);
    in_language("de", || {
        assert_eq!(said.english(), "Imported 3 downloads.");
        assert_eq!(said.shown(), "3 Downloads importiert.");
    });
    assert_eq!(said.shown(), said.english(), "in English, the same");
}

#[test]
fn a_sentence_naming_a_queue_names_it_as_the_tree_does() {
    let said = Said::counting_naming(
        "Started {} download in {}.||Started {} downloads in {}.",
        2,
        &[&2, &dm_core::MAIN_QUEUE],
    );
    in_language("de", || {
        assert!(
            said.shown().contains("Haupt-Warteschlange"),
            "{}",
            said.shown()
        );
        assert_eq!(said.english(), "Started 2 downloads in Main queue.");
    });
}

/// A count takes its language's form, not "(s)".
#[test]
fn a_count_takes_the_form_its_language_gives_it() {
    let added = "Added {} download.||Added {} downloads.";
    let one = |n: u64| tn(added, n, &[&n]);
    assert_eq!(one(1), "Added 1 download.");
    assert_eq!(one(2), "Added 2 downloads.");
    assert_eq!(one(0), "Added 0 downloads.");
    in_language("de", || {
        // The status bar's sentence, in German.
        let running = "{} downloading at {}||{} downloading at {}";
        assert_eq!(
            tn(running, 1, &[&1, &"5 KB/s"]),
            "1 Download läuft mit 5 KB/s"
        );
        assert_eq!(
            tn(running, 3, &[&3, &"5 KB/s"]),
            "3 Downloads laufen mit 5 KB/s"
        );
        assert_eq!(one(1), "1 Download hinzugefügt.");
        assert_eq!(one(4), "4 Downloads hinzugefügt.");
    });
    in_language("ru", || {
        let forms: Vec<String> = [1, 3, 5, 11, 21, 22, 25].map(one).to_vec();
        assert_eq!(forms[0], forms[4].replace("21", "1"), "21 is singular");
        assert_eq!(forms[1], forms[5].replace("22", "3"), "22 is a few");
        assert_eq!(forms[2], forms[3].replace("11", "5"), "11 is many");
        assert_ne!(forms[0], forms[1].replace('3', "1"));
        assert_ne!(forms[1], forms[2].replace('5', "3"));
    });
    assert_eq!(form("pl", 1), 0);
    assert_eq!(form("pl", 22), 1);
    assert_eq!(form("pl", 12), 2);
    assert_eq!(form("fr", 0), 0);
    assert_eq!(form("tr", 7), 0);
    assert_eq!(form("en", 1), 0);
    assert_eq!(form("en", 7), 1);
}

#[test]
fn a_file_called_like_a_built_in_name_is_shown_as_it_is() {
    let said = Said::new("Found {}.", &[&"Videos"]);
    in_language("de", || assert_eq!(said.shown(), "Gefunden: Videos."));
    // Sizes take the language's decimal mark and units, where the
    // computer's regional formats say nothing.
    crate::region::pin(None);
    in_language("de", || {
        assert_eq!(crate::format::bytes(3_984_588), "3,8 MB")
    });
    in_language("ru", || {
        assert_eq!(crate::format::bytes(3_984_588), "3,8 МБ")
    });
    in_language("en", || {
        assert_eq!(crate::format::bytes(3_984_588), "3.8 MB")
    });
    // And the computer's mark where it has one, whatever the language.
    in_language("en", || {
        assert_eq!(crate::format::bytes_marked(3_984_588, Some(',')), "3,8 MB")
    });
}

#[test]
fn a_sentence_made_elsewhere_is_shown_as_it_is() {
    let said = Said::from("the server answered 403");
    in_language("ru", || {
        assert_eq!(said.shown(), "the server answered 403");
        assert_eq!(said.english(), "the server answered 403");
    });
    assert_eq!(Said::from(String::from("x")), Said::from("x"));
}

#[test]
fn an_empty_sentence_is_empty() {
    assert!(Said::default().is_empty());
    assert!(Said::from("").is_empty());
    assert!(!Said::from("x").is_empty());
    assert!(!Said::new("Speed limiter off.", &[]).is_empty());
    assert!(Said::new("", &[]).is_empty());
    assert_eq!(Said::default().shown(), "");
}

/* --------------------------------------------------- catalogs */

#[test]
fn every_catalog_parses_and_is_read_once() {
    for (code, text) in CATALOGS {
        let parsed: Result<HashMap<String, String>, _> = serde_json::from_str(text);
        assert!(parsed.is_ok(), "{code}: {:?}", parsed.err());
        let first = catalog(code).expect("a catalog for every offered language");
        assert!(
            std::ptr::eq(first, catalog(code).unwrap()),
            "{code} read twice"
        );
        assert!(!first.is_empty(), "{code} is empty");
    }
    assert!(catalog("en").is_none(), "English has no catalog");
    assert!(catalog("xx").is_none());
}

#[test]
fn every_language_but_english_has_a_catalog() {
    for (code, _) in LANGUAGES {
        assert_eq!(catalog(code).is_some(), code != "en", "{code}");
    }
}
