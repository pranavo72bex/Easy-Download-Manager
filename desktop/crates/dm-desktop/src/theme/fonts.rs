//! The faces the window draws with, and how they are loaded.

use super::*;

/// The semibold family, for headings and the selected place.
pub fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}

/// Digits of one width, for the list's sizes, speeds and times, so a
/// column of them lines up and a changing speed does not jitter.
/// Letters and units fall through to Inter.
pub fn numbers_family() -> FontFamily {
    FontFamily::Name("numbers".into())
}

/// The numbers family where it is loaded yet, else the regular face, for
/// the reason [`bold`] gives.
pub fn numbers(ctx: &Context) -> FontFamily {
    if fonts_ready(ctx) {
        numbers_family()
    } else {
        FontFamily::Proportional
    }
}

/// Semibold where it is loaded yet, else the regular face. Fonts set on one
/// frame take effect on the next, and egui panics on a family it has not
/// got, which is what dm did on its first frame.
pub fn bold(ctx: &Context) -> FontFamily {
    if fonts_ready(ctx) {
        semibold()
    } else {
        FontFamily::Proportional
    }
}

/// Whether Inter's semibold is in force: set on an earlier frame, which
/// is when egui starts using fonts given to `set_fonts`. Kept as a frame
/// number rather than asked of egui, which panics if asked before its
/// first frame.
pub fn fonts_ready(ctx: &Context) -> bool {
    ctx.data(|d| d.get_temp::<u64>(egui::Id::unique("dm-fonts-pass")))
        .is_some_and(|set| ctx.cumulative_pass_nr() > set)
}

/// Sets the window's fonts once: see [`definitions`].
pub(super) fn fonts(ctx: &Context) {
    let id = egui::Id::unique("dm-fonts-pass");
    if ctx.data(|d| d.get_temp::<u64>(id)).is_some() {
        return;
    }
    ctx.set_fonts(definitions(&scripts_asked(ctx)));
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.insert_temp(id, pass));
}

/// dm's fonts: the window's face (Segoe UI or Selawik) first, Inter
/// behind it, egui's own after them, and the computer's
/// fonts for `scripts` last of all, so each only draws what the others
/// cannot.
pub(super) fn definitions(scripts: &[Script]) -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    // The face WinUI draws with where the computer has it, else its open
    // stand-in; Inter behind it for what it has no letters for.
    let (regular, semibold) = ui_faces();
    fonts.font_data.insert("ui".into(), regular.into());
    fonts
        .font_data
        .insert("ui-semibold".into(), semibold.into());
    fonts.font_data.insert(
        "inter".into(),
        egui::FontData::from_static(include_bytes!("../../fonts/Inter-Regular.ttf")).into(),
    );
    fonts.font_data.insert(
        "inter-semibold".into(),
        egui::FontData::from_static(include_bytes!("../../fonts/Inter-SemiBold.ttf")).into(),
    );
    if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
        family.insert(0, "inter".into());
        family.insert(0, "ui".into());
        // Last: the sort arrows (↑ ↓) are in none of the others, and were
        // drawn as a missing-glyph box until egui 0.37 made that visible.
        family.push("Hack".into());
    }
    for script in scripts {
        let Some(data) = system_font_data(*script) else {
            continue;
        };
        let name = format!("system-{script:?}");
        fonts.font_data.insert(name.clone(), data.into());
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            if let Some(list) = fonts.families.get_mut(&family) {
                list.push(name.clone());
            }
        }
    }
    let mut semibold = vec!["ui-semibold".to_string(), "inter-semibold".to_string()];
    semibold.extend(
        fonts
            .families
            .get(&FontFamily::Proportional)
            .cloned()
            .unwrap_or_default(),
    );
    fonts.families.insert(semibold_family(), semibold);
    // Segoe UI's and Selawik's digits are each one width, so the window's
    // face leads the numbers family as it does every other. Segoe UI
    // Variable's are not (its tabular figures are an OpenType feature egui
    // cannot ask for): there classic Segoe UI draws the numbers.
    let mut numbers = vec!["ui".to_string()];
    if let Some(face) = windows_numbers() {
        fonts.font_data.insert("ui-numbers".into(), face.into());
        numbers.insert(0, "ui-numbers".to_string());
    }
    numbers.extend(
        fonts
            .families
            .get(&FontFamily::Proportional)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|f| f != "ui"),
    );
    fonts.families.insert(numbers_family(), numbers);
    // Fluent UI System Icons, the few the window draws (MIT).
    fonts.font_data.insert(
        "fluent-icons".into(),
        egui::FontData::from_static(include_bytes!("../../fonts/FluentSystemIcons-dm.ttf")).into(),
    );
    fonts
        .families
        .insert(crate::icons::family(), vec!["fluent-icons".to_string()]);
    fonts
}

/// The window's typeface, regular and semibold. Fluent's is Segoe UI,
/// which Microsoft licenses with Windows and nobody may ship: on Windows the
/// computer's own is used, as WinUI does; elsewhere, and on a Windows
/// without it, Selawik, Microsoft's open stand-in made to its measure for
/// other systems (SIL OFL 1.1, `fonts/LICENSE-Selawik.txt`), embedded.
pub(super) fn ui_faces() -> (egui::FontData, egui::FontData) {
    if let Some(found) = windows_segoe() {
        return found;
    }
    (
        egui::FontData::from_static(include_bytes!("../../fonts/Selawik-Regular.ttf")),
        egui::FontData::from_static(include_bytes!("../../fonts/Selawik-Semibold.ttf")),
    )
}

/// Segoe UI and its semibold from the computer's font folder, mapped:
/// Segoe UI Variable, Windows 11's, where it is there, else Segoe UI.
pub(super) fn windows_segoe() -> Option<(egui::FontData, egui::FontData)> {
    if !cfg!(windows) {
        return None;
    }
    let dir = std::path::PathBuf::from(std::env::var_os("WINDIR")?).join("Fonts");
    if let Some(blob) = font_file(&dir.join("SegUIVar.ttf")) {
        return Some(variable_faces(blob));
    }
    let face =
        |name: &str| font_file(&dir.join(name)).map(|blob| egui::FontData::from_blob(blob, 0));
    Some((face("segoeui.ttf")?, face("seguisb.ttf")?))
}

/// Classic Segoe UI, for numbers, where the window's face is Segoe UI
/// Variable, whose digits differ in width.
pub(super) fn windows_numbers() -> Option<egui::FontData> {
    if !cfg!(windows) {
        return None;
    }
    let dir = std::path::PathBuf::from(std::env::var_os("WINDIR")?).join("Fonts");
    if !dir.join("SegUIVar.ttf").is_file() {
        return None;
    }
    font_file(&dir.join("segoeui.ttf")).map(|blob| egui::FontData::from_blob(blob, 0))
}

/// One variable font as the regular and semibold faces: its weight axis
/// at 400 and 600, and its optical size at that of text (Segoe UI
/// Variable's "Text", 10.5), which an axis the font lacks ignores.
pub(super) fn variable_faces(blob: egui::epaint::text::Blob) -> (egui::FontData, egui::FontData) {
    let weighted = |weight: f32| {
        let mut face = egui::FontData::from_blob(blob.clone(), 0);
        face.tweak.coords =
            egui::epaint::text::VariationCoords::new([(b"wght", weight), (b"opsz", 10.5)]);
        face
    };
    (weighted(400.0), weighted(600.0))
}

/* ------------------------------------------- the computer's own fonts */
