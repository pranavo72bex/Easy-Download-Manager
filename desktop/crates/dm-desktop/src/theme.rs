//! dm's looks, as tokens: every colour, radius and shadow the window draws
//! comes from here.
//!
//! The default look is dm's modern one, light or dark as the computer is
//! set: neutral backgrounds, foregrounds and strokes, a blue brand colour,
//! 4 px control and 8 px card radii, and soft shadows. Its typeface is
//! Segoe UI where Windows has it, else Selawik, Microsoft's open stand-in
//! for it (SIL OFL 1.1, `fonts/LICENSE-Selawik.txt`), with Inter behind
//! both for Greek and Cyrillic (SIL OFL 1.1, `fonts/LICENSE-Inter.txt`). A classic look stays as a choice in
//! Options. `docs/ui-design.md` lists the tokens.

use egui::epaint::Shadow;
use egui::{Color32, Context, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Visuals};

use crate::icons::Icon;

/// Which look the window has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    /// The modern look, light.
    Light,
    /// The modern look, dark.
    Dark,
    /// A classic light Windows window.
    Classic,
}

impl Theme {
    /// The theme a settings word chooses. "system" (and anything unknown)
    /// follows the computer: dark when it is dark, else light.
    pub fn resolve(word: &str, system_dark: bool) -> Theme {
        match word.trim() {
            "light" => Theme::Light,
            "dark" => Theme::Dark,
            "classic" => Theme::Classic,
            _ if system_dark => Theme::Dark,
            _ => Theme::Light,
        }
    }

    pub fn tokens(self) -> Tokens {
        match self {
            // The modern look, light: WinUI 3's light brushes (Fluent 2 as
            // Windows draws it), each laid over the surface it sits on.
            Theme::Light => Tokens {
                dark: false,
                modern: true,
                // SolidBackgroundFillColorBase: the window, the menu bar,
                // the command bar and the navigation pane.
                frame: hex(0xf3f3f3),
                pane: hex(0xf3f3f3),
                // LayerFillColorDefault over the base: the list's surface.
                list: hex(0xf9f9f9),
                header: hex(0xf9f9f9),
                // LayerFillColorAlt: a dialog's body.
                dialog: hex(0xffffff),
                // CardStrokeColorDefault and DividerStrokeColorDefault.
                border: hex(0xe5e5e5),
                // ControlStrongStrokeColorDefault: a check box's box, a
                // field's underline; a shade darker than WinUI's #8d8d8d,
                // which is 2.99:1 on the list.
                stroke: hex(0x8a8a8a),
                // ControlElevationBorder: a button's or a field's edge.
                edge: hex(0xe5e5e5),
                // TextFillColorPrimary, Secondary and Disabled.
                text: hex(0x1b1b1b),
                text2: hex(0x1b1b1b),
                weak: hex(0x616161),
                // TextFillColorDisabled over the list (#9f9f9f), a shade
                // darker to hold 2.5:1.
                disabled: hex(0x9a9a9a),
                // SubtleFillColorSecondary: a selected place or row, and
                // a hovered one; the accent indicator tells them apart.
                selection: Color32::from_black_alpha(0x09),
                selection_text: hex(0x1b1b1b),
                row_selected: Color32::from_black_alpha(0x09),
                row_selected_text: hex(0x1b1b1b),
                // FocusStrokeColorOuter and Inner.
                focus_outer: hex(0x1b1b1b),
                focus_inner: Color32::WHITE,
                // SubtleFillColorSecondary and Tertiary, for controls
                // without a fill of their own.
                hover: Color32::from_black_alpha(0x09),
                pressed: Color32::from_black_alpha(0x06),
                // ControlFillColorDefault, Secondary and Tertiary on Mica:
                // a button's or a field's fill.
                control: hex(0xfbfbfb),
                control_hover: hex(0xf6f6f6),
                control_pressed: hex(0xf5f5f5),
                // AccentTextFillColorPrimary: links and words in the accent.
                accent: hex(0x003e92),
                // AccentFillColorDefault, Secondary and Tertiary from the
                // default accent (#0078d4): the primary button, the
                // selection indicator, a checked box, the progress bar.
                brand: hex(0x005fb8),
                brand_hover: hex(0x1a6fbf),
                brand_pressed: hex(0x337fc6),
                // TextOnAccentFillColorPrimary.
                on_brand: Color32::WHITE,
                // The system accent itself, behind selected words in a field.
                text_selection: hex(0x0078d4),
                // SystemFillColorCritical, Success and Caution.
                error: hex(0xc42b1c),
                success: hex(0x0f7b0f),
                warn: hex(0x9d5d00),
                folder: hex(0x1b1b1b),
                coloured_tools: false,
                // ControlCornerRadius and OverlayCornerRadius.
                radius: 4,
                card_radius: 8,
            },
            // The modern look, dark: WinUI 3's dark brushes.
            Theme::Dark => Tokens {
                dark: true,
                modern: true,
                frame: hex(0x202020),
                pane: hex(0x202020),
                list: hex(0x282828),
                header: hex(0x282828),
                dialog: hex(0x2b2b2b),
                border: hex(0x3a3a3a),
                stroke: hex(0x9e9e9e),
                edge: hex(0x3e3e3e),
                text: Color32::WHITE,
                text2: Color32::WHITE,
                weak: hex(0xcfcfcf),
                disabled: hex(0x787878),
                selection: Color32::from_white_alpha(0x0f),
                selection_text: Color32::WHITE,
                row_selected: Color32::from_white_alpha(0x0f),
                row_selected_text: Color32::WHITE,
                focus_outer: Color32::WHITE,
                focus_inner: hex(0x0b0b0b),
                hover: Color32::from_white_alpha(0x0f),
                pressed: Color32::from_white_alpha(0x0a),
                control: hex(0x373737),
                control_hover: hex(0x3c3c3c),
                control_pressed: hex(0x323232),
                accent: hex(0x99ebff),
                brand: hex(0x60cdff),
                brand_hover: hex(0x5bbdea),
                brand_pressed: hex(0x55acd4),
                on_brand: Color32::BLACK,
                text_selection: hex(0x0078d4),
                error: hex(0xff99a4),
                success: hex(0x6ccb5f),
                warn: hex(0xfce100),
                folder: Color32::WHITE,
                coloured_tools: false,
                radius: 4,
                card_radius: 8,
            },
            // the classic look: a plain light desktop palette.
            Theme::Classic => Tokens {
                dark: false,
                modern: false,
                frame: hex(0xf0f0f0),
                pane: hex(0xf0f0f0),
                list: Color32::WHITE,
                header: hex(0xf7f7f7),
                dialog: Color32::WHITE,
                border: hex(0xd4d4d4),
                stroke: hex(0x8a8a8a),
                edge: hex(0x8a8a8a),
                text: hex(0x1a1a1a),
                text2: hex(0x1a1a1a),
                weak: hex(0x6d6d6d),
                disabled: hex(0x8f8f8f),
                selection: hex(0xa9d1f5),
                selection_text: hex(0x1a1a1a),
                row_selected: hex(0x0072d0),
                row_selected_text: Color32::WHITE,
                focus_outer: hex(0x0078d7),
                focus_inner: Color32::WHITE,
                hover: hex(0xd9ebf9),
                pressed: hex(0xb8dcf8),
                control: Color32::WHITE,
                control_hover: hex(0xd9ebf9),
                control_pressed: hex(0xb8dcf8),
                accent: hex(0x0078d7),
                brand: hex(0x0078d7),
                brand_hover: hex(0x005a9e),
                brand_pressed: hex(0x005a9e),
                on_brand: Color32::WHITE,
                text_selection: hex(0x0078d7),
                error: hex(0xc42b1c),
                success: hex(0x107c10),
                warn: hex(0x9c5006),
                folder: hex(0xf2c24b),
                coloured_tools: true,
                radius: 2,
                card_radius: 4,
            },
        }
    }
}

impl Tokens {
    /// The progress bar's fill: the accent fill, which each look already
    /// makes stand out on its own list.
    pub fn progress(&self) -> Color32 {
        self.brand
    }
}

/// How far apart two colours are to the eye, as WCAG measures it: 1 for
/// the same colour, 21 for black on white.
pub fn contrast(a: Color32, b: Color32) -> f32 {
    fn channel(c: u8) -> f32 {
        let c = c as f32 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    let luminance =
        |c: Color32| 0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b());
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// Every colour and radius the window draws with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tokens {
    pub dark: bool,
    /// The modern layout: a toolbar, a navigation pane with an indicator,
    /// the list on a card, thin progress bars.
    pub modern: bool,
    /// The window behind everything: menus, command bar.
    pub frame: Color32,
    /// The navigation pane.
    pub pane: Color32,
    /// The download list's card.
    pub list: Color32,
    pub header: Color32,
    /// A dialog's body.
    pub dialog: Color32,
    /// Hairlines and the card's edge.
    pub border: Color32,
    /// Strong outlines: a check box's box, a field's underline.
    pub stroke: Color32,
    /// The soft edge of a button or a field.
    pub edge: Color32,
    pub text: Color32,
    pub text2: Color32,
    pub weak: Color32,
    pub disabled: Color32,
    /// The selected place in the navigation pane.
    pub selection: Color32,
    pub selection_text: Color32,
    /// A selected row of the download list, and its words: a fill that
    /// stands out from the list at 3:1 or more, words on it at 4.5:1.
    pub row_selected: Color32,
    pub row_selected_text: Color32,
    /// The focus ring: two lines, the outer one in the text colour and an
    /// inner one in the background's, so it shows on any fill.
    pub focus_outer: Color32,
    pub focus_inner: Color32,
    /// Hover and press on a control without a fill of its own.
    pub hover: Color32,
    pub pressed: Color32,
    /// A button's or a field's fill, at rest, hovered and pressed.
    pub control: Color32,
    pub control_hover: Color32,
    pub control_pressed: Color32,
    /// Links and words in the accent colour.
    pub accent: Color32,
    /// The accent fill: the primary button, the selection indicator, a
    /// checked box, the progress bar.
    pub brand: Color32,
    pub brand_hover: Color32,
    pub brand_pressed: Color32,
    pub on_brand: Color32,
    /// Behind selected words in a field.
    pub text_selection: Color32,
    pub error: Color32,
    pub success: Color32,
    pub warn: Color32,
    /// The navigation pane's pictures.
    pub folder: Color32,
    /// The classic look draws its toolbar pictures in colour; the modern one in the text colour.
    pub coloured_tools: bool,
    pub radius: u8,
    pub card_radius: u8,
}

impl Tokens {
    /// The tokens in force, as `apply` left them.
    pub fn get(ctx: &Context) -> Tokens {
        ctx.data(|d| d.get_temp::<Tokens>(egui::Id::unique("dm-tokens")))
            .unwrap_or_else(|| Theme::Light.tokens())
    }

    /// The colour of a toolbar picture.
    pub fn tool(&self, icon: Icon, enabled: bool) -> Color32 {
        if !enabled {
            return self.disabled;
        }
        if !self.coloured_tools {
            return self.text2;
        }
        match icon {
            Icon::Add => Color32::from_rgb(0x2e, 0x9e, 0x3e),
            Icon::Resume | Icon::StartQueue => Color32::from_rgb(0x1f, 0x7a, 0xd6),
            Icon::Stop | Icon::StopAll | Icon::StopQueue => Color32::from_rgb(0xd1, 0x3b, 0x2f),
            Icon::Delete | Icon::DeleteCompleted => Color32::from_rgb(0xb8, 0x3a, 0x3a),
            Icon::Options => Color32::from_rgb(0x55, 0x5f, 0x6b),
            Icon::Scheduler => Color32::from_rgb(0x8a, 0x5a, 0x00),
            Icon::SiteFiles => Color32::from_rgb(0x0e, 0x8a, 0x8a),
        }
    }
}

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
fn fonts(ctx: &Context) {
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
fn definitions(scripts: &[Script]) -> egui::FontDefinitions {
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
        egui::FontData::from_static(include_bytes!("../fonts/Inter-Regular.ttf")).into(),
    );
    fonts.font_data.insert(
        "inter-semibold".into(),
        egui::FontData::from_static(include_bytes!("../fonts/Inter-SemiBold.ttf")).into(),
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
        egui::FontData::from_static(include_bytes!("../fonts/FluentSystemIcons-dm.ttf")).into(),
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
fn ui_faces() -> (egui::FontData, egui::FontData) {
    if let Some(found) = windows_segoe() {
        return found;
    }
    (
        egui::FontData::from_static(include_bytes!("../fonts/Selawik-Regular.ttf")),
        egui::FontData::from_static(include_bytes!("../fonts/Selawik-Semibold.ttf")),
    )
}

/// Segoe UI and its semibold from the computer's font folder, mapped:
/// Segoe UI Variable, Windows 11's, where it is there, else Segoe UI.
fn windows_segoe() -> Option<(egui::FontData, egui::FontData)> {
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
fn windows_numbers() -> Option<egui::FontData> {
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
fn variable_faces(blob: egui::epaint::text::Blob) -> (egui::FontData, egui::FontData) {
    let weighted = |weight: f32| {
        let mut face = egui::FontData::from_blob(blob.clone(), 0);
        face.tweak.coords =
            egui::epaint::text::VariationCoords::new([(b"wght", weight), (b"opsz", 10.5)]);
        face
    };
    (weighted(400.0), weighted(600.0))
}

/* ------------------------------------------- the computer's own fonts */

/// Writing systems Inter and egui's fonts have no letters for, which a
/// file name can still be in. Each is drawn with a font the computer
/// already has, read the first time a name needs it: nothing is bundled,
/// and a computer without one draws what it can, as before.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Script {
    /// Chinese characters, and Japanese with its kana.
    Cjk,
    Hangul,
    Arabic,
    Hebrew,
    Thai,
    Devanagari,
}

impl Script {
    pub const ALL: [Script; 6] = [
        Script::Cjk,
        Script::Hangul,
        Script::Arabic,
        Script::Hebrew,
        Script::Thai,
        Script::Devanagari,
    ];

    /// The writing system a character needs a font for, if any.
    pub fn of(c: char) -> Option<Script> {
        match u32::from(c) {
            0x0590..=0x05FF | 0xFB1D..=0xFB4F => Some(Script::Hebrew),
            0x0600..=0x06FF | 0x0750..=0x077F | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF => {
                Some(Script::Arabic)
            }
            0x0900..=0x097F | 0xA8E0..=0xA8FF => Some(Script::Devanagari),
            0x0E00..=0x0E7F => Some(Script::Thai),
            0x1100..=0x11FF | 0x3130..=0x318F | 0xA960..=0xA97F | 0xAC00..=0xD7FF => {
                Some(Script::Hangul)
            }
            0x2E80..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF | 0x20000..=0x3FFFF => {
                Some(Script::Cjk)
            }
            _ => None,
        }
    }

    /// The language fontconfig is asked about, for this script.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    fn language(self) -> &'static str {
        match self {
            Script::Cjk => "zh-cn",
            Script::Hangul => "ko",
            Script::Arabic => "ar",
            Script::Hebrew => "he",
            Script::Thai => "th",
            Script::Devanagari => "hi",
        }
    }

    /// Where this computer's fonts for the script usually are, best first.
    fn known_files(self) -> &'static [&'static str] {
        #[cfg(target_os = "macos")]
        return match self {
            Script::Cjk => &[
                "/System/Library/Fonts/PingFang.ttc",
                "/System/Library/Fonts/Hiragino Sans GB.ttc",
                "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
                "/System/Library/Fonts/STHeiti Light.ttc",
            ],
            Script::Hangul => &[
                "/System/Library/Fonts/AppleSDGothicNeo.ttc",
                "/System/Library/Fonts/Supplemental/AppleGothic.ttf",
            ],
            Script::Arabic => &[
                "/System/Library/Fonts/SFArabic.ttf",
                "/System/Library/Fonts/GeezaPro.ttc",
            ],
            Script::Hebrew => &[
                "/System/Library/Fonts/SFHebrew.ttf",
                "/System/Library/Fonts/ArialHB.ttc",
            ],
            Script::Thai => &[
                "/System/Library/Fonts/ThonburiUI.ttc",
                "/System/Library/Fonts/Thonburi.ttc",
                "/System/Library/Fonts/Supplemental/Thonburi.ttc",
            ],
            Script::Devanagari => &[
                "/System/Library/Fonts/Kohinoor.ttc",
                "/System/Library/Fonts/Supplemental/DevanagariMT.ttc",
            ],
        };
        #[cfg(windows)]
        return match self {
            Script::Cjk => &[
                "C:\\Windows\\Fonts\\msyh.ttc",
                "C:\\Windows\\Fonts\\YuGothM.ttc",
                "C:\\Windows\\Fonts\\meiryo.ttc",
                "C:\\Windows\\Fonts\\simsun.ttc",
            ],
            Script::Hangul => &[
                "C:\\Windows\\Fonts\\malgun.ttf",
                "C:\\Windows\\Fonts\\gulim.ttc",
            ],
            Script::Arabic | Script::Hebrew => &[
                "C:\\Windows\\Fonts\\tahoma.ttf",
                "C:\\Windows\\Fonts\\arial.ttf",
            ],
            Script::Thai => &[
                "C:\\Windows\\Fonts\\LeelawUI.ttf",
                "C:\\Windows\\Fonts\\tahoma.ttf",
            ],
            Script::Devanagari => &[
                "C:\\Windows\\Fonts\\Nirmala.ttf",
                "C:\\Windows\\Fonts\\mangal.ttf",
            ],
        };
        #[cfg(not(any(target_os = "macos", windows)))]
        return match self {
            Script::Cjk => &[
                "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
                "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
                "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
                "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
                "/usr/share/fonts/wenquanyi/wqy-microhei/wqy-microhei.ttc",
            ],
            Script::Hangul => &[
                "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
                "/usr/share/fonts/truetype/nanum/NanumGothic.ttf",
            ],
            Script::Arabic => &["/usr/share/fonts/truetype/noto/NotoSansArabic-Regular.ttf"],
            Script::Hebrew => &["/usr/share/fonts/truetype/noto/NotoSansHebrew-Regular.ttf"],
            Script::Thai => &["/usr/share/fonts/truetype/noto/NotoSansThai-Regular.ttf"],
            Script::Devanagari => {
                &["/usr/share/fonts/truetype/noto/NotoSansDevanagari-Regular.ttf"]
            }
        };
    }
}

/// The computer's font file for `script`, and the face in it: on Linux,
/// whichever fontconfig says covers the language; elsewhere, and failing
/// that, the first of the usual files that is there.
pub fn system_font(script: Script) -> Option<(std::path::PathBuf, u32)> {
    // A Japanese window draws its kanji in a Japanese face: a Chinese one
    // shapes some of the same characters differently.
    let japanese = script == Script::Cjk && crate::i18n::code() == "ja";
    #[cfg(target_os = "linux")]
    if let Some(found) = fontconfig(script, japanese) {
        return Some(found);
    }
    let first: &[&str] = if japanese { JAPANESE_FILES } else { &[] };
    first
        .iter()
        .chain(script.known_files())
        .map(std::path::PathBuf::from)
        .find(|p| p.is_file())
        .map(|p| (p, 0))
}

/// Japanese faces, tried first for a Japanese window's kanji and kana.
#[cfg(target_os = "macos")]
const JAPANESE_FILES: &[&str] = &[
    "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
    "/System/Library/Fonts/Hiragino Sans W3.ttc",
];
#[cfg(windows)]
const JAPANESE_FILES: &[&str] = &[
    "C:\\Windows\\Fonts\\YuGothM.ttc",
    "C:\\Windows\\Fonts\\meiryo.ttc",
];
#[cfg(not(any(target_os = "macos", windows)))]
const JAPANESE_FILES: &[&str] = &[];

/// Asks fontconfig for a sans font that covers `script`'s language, and
/// takes it only if it does: `fc-match` always answers with something.
#[cfg(target_os = "linux")]
fn fontconfig(script: Script, japanese: bool) -> Option<(std::path::PathBuf, u32)> {
    let language = if japanese { "ja" } else { script.language() };
    let out = std::process::Command::new("fc-match")
        .arg("-f")
        .arg("%{file}\\n%{index}\\n%{lang}")
        .arg(format!("sans-serif:lang={language}"))
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    let mut lines = text.lines();
    let file = std::path::PathBuf::from(lines.next()?);
    let index = lines.next()?.trim().parse().unwrap_or(0);
    let covers = lines
        .next()?
        .split('|')
        .any(|l| l == language || l.split('-').next() == language.split('-').next());
    (covers && file.is_file()).then_some((file, index))
}

/// The font for `script`, read from disk once a process; `None` where the
/// computer has none (asked once too).
fn system_font_data(script: Script) -> Option<egui::FontData> {
    static READ: [std::sync::OnceLock<Option<egui::FontData>>; 6] =
        [const { std::sync::OnceLock::new() }; 6];
    let at = Script::ALL.iter().position(|s| *s == script)?;
    READ.get(at)?
        .get_or_init(|| {
            let (path, index) = system_font(script)?;
            Some(egui::FontData::from_blob(font_file(&path)?, index))
        })
        .clone()
}

/// A font file of the computer's, mapped into memory rather than read
/// into it: a CJK collection is about 20 MB, all of it held for a few
/// glyphs, where mapped only the pages read are in memory and the system
/// can drop them again (+19 MB for one Japanese
/// name). Read whole where it cannot be mapped.
fn font_file(path: &std::path::Path) -> Option<egui::epaint::text::Blob> {
    let file = std::fs::File::open(path).ok()?;
    // SAFETY: a system font, which package managers and system updates
    // replace by putting a new file in its place, leaving this mapping
    // whole; macOS keeps its own fonts on a read-only volume. A font cut
    // short where it lies, under a running dm, is the case this does not
    // survive.
    match unsafe { memmap2::Mmap::map(&file) } {
        Ok(map) => Some(std::sync::Arc::new(map)),
        Err(_) => Some(std::sync::Arc::new(std::fs::read(path).ok()?)),
    }
}

/// The scripts asked for so far in this window.
fn scripts_asked(ctx: &Context) -> Vec<Script> {
    ctx.data(|d| d.get_temp::<Vec<Script>>(egui::Id::unique("dm-scripts")))
        .unwrap_or_default()
}

/// Makes sure every script in `texts` has a font: the first name in
/// Chinese, Korean, Arabic… has the computer's font for it read and added
/// behind dm's own, to be drawn from the next frame. Cheap when nothing is
/// new, which is every frame but the first that needs one.
pub fn cover<'a>(ctx: &Context, texts: impl IntoIterator<Item = &'a str>) {
    let mut asked = scripts_asked(ctx);
    if asked.len() == Script::ALL.len() {
        return;
    }
    let before = asked.len();
    for text in texts {
        if text.is_ascii() {
            continue;
        }
        for script in text.chars().filter_map(Script::of) {
            if !asked.contains(&script) {
                asked.push(script);
            }
        }
    }
    if asked.len() == before {
        return;
    }
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique("dm-scripts"), asked.clone()));
    ctx.set_fonts(definitions(&asked));
}

fn semibold_family() -> FontFamily {
    semibold()
}

/// Puts `theme` in force: egui's visuals and spacing from its tokens, the
/// type sizes, and the tokens where widgets read them.
pub fn apply(ctx: &Context, theme: Theme) {
    apply_with_accent(ctx, theme, None);
}

/// [`apply`], the modern looks' accent brushes taken from `accent` (the
/// computer's), where it reads well.
pub fn apply_with_accent(ctx: &Context, theme: Theme, accent: Option<crate::accent::Palette>) {
    fonts(ctx);
    let mut t = match &accent {
        Some(palette) => crate::accent::tokens(theme.tokens(), palette),
        None => theme.tokens(),
    };
    // Under Mica the frame is clear and the backdrop is its colour; the
    // list, dialogs and fields stay solid to be read on.
    if crate::chrome::mica() {
        t.frame = egui::Color32::TRANSPARENT;
    }
    let mut v = if t.dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    v.panel_fill = t.frame;
    v.window_fill = t.dialog;
    v.extreme_bg_color = t.control;
    v.faint_bg_color = t.header;
    v.code_bg_color = t.pane;
    v.override_text_color = Some(t.text);
    v.selection.bg_fill = t.selection;
    v.selection.stroke = Stroke::new(1.0, t.selection_text);
    v.hyperlink_color = t.accent;
    v.error_fg_color = t.error;
    v.warn_fg_color = t.warn;
    v.window_stroke = Stroke::new(1.0, t.border);
    // The navigation pane and the places tree draw no guide lines down
    // their indents, as a navigation view does not.
    v.indent_has_left_vline = !t.modern;
    let radius = CornerRadius::same(t.radius);
    v.window_corner_radius = CornerRadius::same(t.card_radius);
    v.menu_corner_radius = CornerRadius::same(t.card_radius);
    // Fluent's elevation: a dialog at 28, a menu or a tooltip at 16 (a key
    // and an ambient shadow, here as the one egui draws). Not 64: egui
    // counts a window's shadow in its size, and at 200% a dialog with that
    // much shadow no longer fitted the window.
    let (deep, soft) = if t.dark { (71, 71) } else { (61, 36) };
    v.window_shadow = Shadow {
        offset: [0, 14],
        blur: 28,
        spread: 0,
        color: Color32::from_black_alpha(deep),
    };
    v.popup_shadow = Shadow {
        offset: [0, 8],
        blur: 16,
        spread: 0,
        color: Color32::from_black_alpha(soft),
    };
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = radius;
        w.expansion = 0.0;
    }
    // A button: the control fill with its soft edge; hovered and pressed
    // in the control's own lighter fills, and pressed words in the
    // secondary colour. Controls without a fill of their own (a menu's
    // items, the command bar) take the subtle fills, `weak_bg_fill`.
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, t.border);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, t.text);
    v.widgets.inactive.bg_fill = t.control;
    v.widgets.inactive.weak_bg_fill = t.control;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, t.edge);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, t.text);
    v.widgets.hovered.bg_fill = t.control_hover;
    v.widgets.hovered.weak_bg_fill = if t.modern { t.control_hover } else { t.hover };
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, t.edge);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, t.text);
    v.widgets.active.bg_fill = t.control_pressed;
    v.widgets.active.weak_bg_fill = if t.modern {
        t.control_pressed
    } else {
        t.pressed
    };
    v.widgets.active.bg_stroke = Stroke::new(1.0, t.edge);
    v.widgets.active.fg_stroke = Stroke::new(1.0, if t.modern { t.weak } else { t.text });
    v.widgets.open.bg_fill = t.control_pressed;
    v.widgets.open.weak_bg_fill = t.selection;
    v.widgets.open.bg_stroke = Stroke::new(1.0, t.edge);
    v.widgets.open.fg_stroke = Stroke::new(1.0, t.text);
    if !t.modern {
        // The classic look as it was: its controls on the list's white,
        // outlined in the strong stroke.
        v.window_fill = t.list;
        v.extreme_bg_color = t.list;
        v.widgets.hovered.bg_fill = t.hover;
        v.widgets.active.bg_fill = t.pressed;
        v.widgets.open.bg_fill = t.selection;
        for w in [
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
        ] {
            w.bg_stroke = Stroke::new(1.0, t.stroke);
        }
    }

    // Pinned, so the system's own light/dark switch does not swap it; the
    // "system" choice is resolved before this, from the computer's setting.
    let which = if t.dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    };
    ctx.set_theme(which);
    ctx.set_visuals_of(which, v);
    let heading = bold(ctx);
    ctx.style_mut_of(which, |style| {
        // Labels are words to read, not text to select: the status bar,
        // a folder's path and a field's name showed the text cursor and
        // could be dragged over. Fields keep theirs.
        style.interaction.selectable_labels = false;
        // The type ramp: Body 14, Caption 12, and Subtitle (20
        // semibold) for a dialog's title.
        style.text_styles = [
            (
                TextStyle::Small,
                FontId::new(12.0, FontFamily::Proportional),
            ),
            (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(14.0, FontFamily::Proportional),
            ),
            (TextStyle::Heading, FontId::new(20.0, heading.clone())),
            (
                TextStyle::Monospace,
                FontId::new(13.0, FontFamily::Monospace),
            ),
        ]
        .into();
        // From egui's own each time: Light's roomier spacing stayed when a
        // running dm went to Classic, which only leaves it alone.
        style.spacing = egui::style::Spacing::default();
        // Every scroll area's bar seen without the pointer, with its own
        // room: Properties at 200% and the progress window at the smallest
        // size cut their last fields off with no sign of more.
        // Set after the line above, which put egui's own back over it.
        style.spacing.scroll = steady_scroll();
        if t.modern {
            // WinUI's compact density, which it offers for information-
            // dense apps: fields and drop-downs 24 tall, buttons padded 11
            // by 5; 8 between controls, a dialog padded 24, a check box's
            // box 20 with 8 before its words.
            style.spacing.button_padding = egui::vec2(11.0, 5.0);
            style.spacing.item_spacing = egui::vec2(8.0, 8.0);
            style.spacing.interact_size.y = 24.0;
            style.spacing.menu_margin = egui::Margin::same(4);
            style.spacing.window_margin = egui::Margin::same(24);
            // A drop-down's chevron takes a square this wide: at 20, a
            // drop-down stood 30 tall beside 27 for a button. The check box's 20 is set where one is drawn
            // (`dialogs::check`).
            style.spacing.icon_width = 16.0;
            style.spacing.icon_width_inner = 10.0;
            style.spacing.icon_spacing = 8.0;
        }
    });
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique("dm-tokens"), t));
}

#[cfg(test)]
mod variable_tests {
    use super::*;

    /// A variable font's two faces are drawn at two weights: wider at 600.
    /// Run where the computer has one (macOS's system font); Segoe UI
    /// Variable itself is only on Windows 11.
    #[test]
    fn a_variable_fonts_faces_are_drawn_at_their_weights() {
        let Some(blob) = font_file(std::path::Path::new("/System/Library/Fonts/SFNS.ttf")) else {
            return;
        };
        let (regular, semibold) = variable_faces(blob);
        let width = |face: egui::FontData| {
            let ctx = egui::Context::default();
            let mut fonts = egui::FontDefinitions::empty();
            fonts
                .font_data
                .insert("face".into(), std::sync::Arc::new(face));
            fonts
                .families
                .insert(egui::FontFamily::Proportional, vec!["face".into()]);
            ctx.set_fonts(fonts);
            // The fonts are in force from the next frame; each frame's
            // texture changes are let go, there being nothing to draw on.
            let mut out = ctx.run_ui(Default::default(), |_| {});
            out.textures_delta.clear();
            let mut drawn = 0.0;
            let mut out = ctx.run_ui(Default::default(), |ui| {
                drawn = ui.fonts_mut(|f| {
                    f.layout_no_wrap(
                        "Download complete".into(),
                        egui::FontId::proportional(14.0),
                        Color32::WHITE,
                    )
                    .size()
                    .x
                });
            });
            out.textures_delta.clear();
            drawn
        };
        let (thin, bold) = (width(regular), width(semibold));
        assert!(bold > thin, "600 drew {bold}, 400 drew {thin}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_look_is_modern_and_follows_the_computer() {
        assert_eq!(Theme::resolve("system", false), Theme::Light);
        assert_eq!(Theme::resolve("system", true), Theme::Dark);
        assert_eq!(Theme::resolve("", true), Theme::Dark, "unknown is system");
        assert_eq!(Theme::resolve("light", true), Theme::Light);
        assert_eq!(Theme::resolve("dark", false), Theme::Dark);
        assert_eq!(Theme::resolve("classic", true), Theme::Classic);
    }

    /// A fill as it shows on `surface`: the subtle fills are see-through.
    fn over(fill: Color32, surface: Color32) -> Color32 {
        let a = fill.a() as f32 / 255.0;
        // Premultiplied: the fill's channels already carry its alpha.
        let mix = |f: u8, s: u8| (f as f32 + s as f32 * (1.0 - a)).round() as u8;
        Color32::from_rgb(
            mix(fill.r(), surface.r()),
            mix(fill.g(), surface.g()),
            mix(fill.b(), surface.b()),
        )
    }

    #[test]
    fn the_modern_looks_are_winui_3s_brushes() {
        let light = Theme::Light.tokens();
        assert_eq!(light.frame, hex(0xf3f3f3), "SolidBackgroundFillColorBase");
        assert_eq!(light.brand, hex(0x005fb8), "AccentFillColorDefault");
        assert_eq!(light.text, hex(0x1b1b1b), "TextFillColorPrimary");
        assert_eq!(
            over(light.hover, light.frame),
            hex(0xeaeaea),
            "SubtleFillColorSecondary"
        );
        assert_eq!((light.radius, light.card_radius), (4, 8));
        let dark = Theme::Dark.tokens();
        assert_eq!(dark.frame, hex(0x202020), "SolidBackgroundFillColorBase");
        assert_eq!(dark.brand, hex(0x60cdff), "AccentFillColorDefault");
        assert_eq!(
            dark.on_brand,
            Color32::BLACK,
            "TextOnAccentFillColorPrimary"
        );
        assert_eq!(
            over(dark.hover, dark.frame),
            hex(0x2d2d2d),
            "SubtleFillColorSecondary"
        );
    }

    /// The focus ring and the selection can be seen in every look (the ring was the accent drawn on an accent button, and the
    /// selection 235 on 255). The ring is two lines, one of which shows on
    /// anything; a selected row or place carries the accent indicator at
    /// 3:1, its words at 4.5:1 on its fill.
    #[test]
    fn the_focus_ring_and_the_selection_can_be_seen_in_every_look() {
        for theme in [Theme::Light, Theme::Dark, Theme::Classic] {
            let t = theme.tokens();
            let selected = over(t.row_selected, t.list);
            let words = contrast(t.row_selected_text, selected);
            assert!(words >= 4.5, "{theme:?}: words on the selection {words}");
            let marked = if t.modern {
                contrast(t.brand, selected)
            } else {
                contrast(selected, t.list)
            };
            assert!(marked >= 3.0, "{theme:?}: the selection's mark {marked}");
            for surface in [t.list, t.frame, t.dialog, t.brand, selected] {
                let seen = contrast(t.focus_outer, surface).max(contrast(t.focus_inner, surface));
                assert!(seen >= 3.0, "{theme:?}: ring on {surface:?} {seen}");
            }
        }
        assert!((contrast(Color32::BLACK, Color32::WHITE) - 21.0).abs() < 0.01);
    }

    /// Hovering and pressing change what a control looks like on every
    /// surface it sits on: the light look's hover was once the frame's own
    /// grey. WinUI's fills are subtle by design, so this asks for a change,
    /// not for contrast.
    #[test]
    fn hover_and_press_can_be_seen_on_every_surface() {
        for theme in [Theme::Light, Theme::Dark, Theme::Classic] {
            let t = theme.tokens();
            for surface in [t.frame, t.pane, t.list, t.dialog] {
                for (what, fill) in [("hover", t.hover), ("pressed", t.pressed)] {
                    assert_ne!(over(fill, surface), surface, "{theme:?}: {what}");
                }
            }
            assert_ne!(t.hover, t.pressed, "{theme:?}: hover and pressed alike");
            assert_ne!(t.control, t.control_hover, "{theme:?}: a button's hover");
            assert_ne!(t.brand, t.brand_hover, "{theme:?}: the primary's hover");
        }
    }

    /// Words and the strong outlines can be read in every look: status
    /// colours and secondary words at 4.5:1 on every surface, a check
    /// box's box, a field's underline and the progress bar at 3:1, and
    /// disabled words still visible (a design review found outlines at
    /// 1.5:1 and the dark error colour at 4.2:1).
    #[test]
    fn status_words_outlines_and_progress_can_be_read_in_every_look() {
        for theme in [Theme::Light, Theme::Dark, Theme::Classic] {
            let t = theme.tokens();
            for (what, colour, floor) in [
                ("words", t.text, 4.5),
                ("error", t.error, 4.5),
                ("warning", t.warn, 4.5),
                ("success", t.success, 4.5),
                ("weak words", t.weak, 4.5),
                ("an outline", t.stroke, 3.0),
                ("the progress bar", t.progress(), 3.0),
                ("disabled words", t.disabled, 2.5),
            ] {
                for surface in [t.list, t.pane, t.dialog] {
                    let got = contrast(colour, surface);
                    assert!(got >= floor, "{theme:?}: {what} {got:.2} < {floor}");
                }
            }
            // The classic look's own blue holds 4.49:1, as it always has.
            let on = contrast(t.on_brand, t.brand);
            let floor = if t.modern { 4.5 } else { 4.4 };
            assert!(
                on >= floor,
                "{theme:?}: words on the primary button {on:.2}"
            );
            let picked = contrast(Color32::WHITE, t.text_selection);
            assert!(
                picked >= floor,
                "{theme:?}: selected words in a field {picked:.2}"
            );
        }
    }

    #[test]
    fn applying_a_theme_puts_its_tokens_where_widgets_read_them() {
        let ctx = Context::default();
        apply(&ctx, Theme::Dark);
        assert_eq!(Tokens::get(&ctx), Theme::Dark.tokens());
        assert!(ctx.global_style().visuals.dark_mode);
        apply(&ctx, Theme::Light);
        assert_eq!(Tokens::get(&ctx), Theme::Light.tokens());
        assert!(!ctx.global_style().visuals.dark_mode);
    }

    #[test]
    fn going_from_a_modern_look_to_classic_leaves_none_of_its_spacing() {
        let ctx = Context::default();
        apply(&ctx, Theme::Light);
        assert_eq!(ctx.global_style().spacing.interact_size.y, 24.0);
        apply(&ctx, Theme::Classic);
        let fresh = egui::style::Spacing::default();
        let now = ctx.global_style().spacing.clone();
        assert_eq!(now.button_padding, fresh.button_padding);
        assert_eq!(now.item_spacing, fresh.item_spacing);
        assert_eq!(now.interact_size, fresh.interact_size);
        assert_eq!(now.window_margin, fresh.window_margin);
        // Every look keeps dm's scroll bars, which egui's own spacing does
        // not have (they were set, then reset).
        for theme in [Theme::Light, Theme::Dark, Theme::Classic] {
            apply(&ctx, theme);
            let scroll = ctx.global_style().spacing.scroll;
            assert_eq!(
                scroll.floating_allocated_width,
                steady_scroll().floating_allocated_width,
                "{theme:?}"
            );
            assert_eq!(
                scroll.dormant_handle_opacity,
                steady_scroll().dormant_handle_opacity,
                "{theme:?}"
            );
        }
    }

    /// Sizes, speeds and times are drawn with digits of one width,
    /// so they line up down a column; the text around them is still Inter's.
    #[test]
    fn numbers_are_drawn_with_digits_of_one_width() {
        let ctx = Context::default();
        let frame = |ctx: &Context, run: &dyn Fn(&Context)| {
            let mut out = ctx.run_ui(Default::default(), |ui| run(ui.ctx()));
            out.textures_delta.clear();
        };
        frame(&ctx, &|ctx| apply(ctx, Theme::Light));
        frame(&ctx, &|_| {});
        assert_eq!(numbers(&ctx), numbers_family());
        let width = |family: FontFamily, c: char| {
            ctx.fonts_mut(|f| f.glyph_width(&FontId::new(14.0, family.clone()), c))
        };
        let ones = width(numbers_family(), '1');
        for c in "023456789".chars() {
            assert_eq!(width(numbers_family(), c), ones, "{c} is not as wide as 1");
        }
        // The window's face is first in both, so the two agree, but where
        // Segoe UI Variable is the face: its digits differ in width, and
        // classic Segoe UI draws the numbers, a close match.
        let (text, number) = (
            width(FontFamily::Proportional, '8'),
            width(numbers_family(), '8'),
        );
        if windows_numbers().is_some() {
            assert!((text - number).abs() <= 1.5, "{text} against {number}");
        } else {
            assert_eq!(text, number);
        }
        // Letters and units are not lost: Cyrillic comes from Inter, behind
        // the window's face.
        assert!(ctx.fonts_mut(|f| f.has_glyphs(&FontId::new(14.0, numbers_family()), "MB/s ГБ")));
        assert_eq!(
            width(numbers_family(), 'M'),
            width(FontFamily::Proportional, 'M')
        );
    }

    /// A Russian dialog's title is bold like an English one; the
    /// earlier face had no Cyrillic, so its titles fell back to a regular one.
    #[test]
    fn titles_in_cyrillic_and_greek_are_bold() {
        let ctx = Context::default();
        let frame = |ctx: &Context, run: &dyn Fn(&Context)| {
            let mut out = ctx.run_ui(Default::default(), |ui| run(ui.ctx()));
            out.textures_delta.clear();
        };
        frame(&ctx, &|ctx| apply(ctx, Theme::Light));
        frame(&ctx, &|_| {});
        let width = |family: FontFamily, text: &str| {
            text.chars()
                .map(|c| ctx.fonts_mut(|f| f.glyph_width(&FontId::new(18.0, family.clone()), c)))
                .sum::<f32>()
        };
        for title in ["Download", "Загрузка", "Λήψη"] {
            assert!(
                width(bold(&ctx), title) > width(FontFamily::Proportional, title),
                "{title} is not drawn bolder in the bold family"
            );
        }
    }

    /// A name in Chinese and Japanese is drawn with the computer's
    /// own font, read once a name needs it; before, its letters were blanks.
    #[test]
    fn a_name_in_chinese_or_japanese_has_letters_to_draw() {
        assert_eq!(Script::of('名'), Some(Script::Cjk));
        assert_eq!(Script::of('フ'), Some(Script::Cjk));
        assert_eq!(Script::of('한'), Some(Script::Hangul));
        assert_eq!(Script::of('é'), None);
        if system_font(Script::Cjk).is_none() {
            eprintln!("this computer has no font for Chinese or Japanese; nothing to check");
            return;
        }
        let ctx = Context::default();
        let frame = |ctx: &Context, run: &dyn Fn(&Context)| {
            let mut out = ctx.run_ui(Default::default(), |ui| run(ui.ctx()));
            out.textures_delta.clear();
        };
        frame(&ctx, &|ctx| apply(ctx, Theme::Light));
        frame(&ctx, &|_| {});
        let font = FontId::proportional(14.0);
        assert!(
            !ctx.fonts_mut(|f| f.has_glyphs(&font, "名前")),
            "drawn before any font for it was read"
        );
        frame(&ctx, &|ctx| cover(ctx, ["report.pdf", "名前ファイル.zip"]));
        frame(&ctx, &|_| {});
        assert!(ctx.fonts_mut(|f| f.has_glyphs(&font, "名前ファイル")));
        assert!(ctx.fonts_mut(|f| f.has_glyphs(&FontId::monospace(14.0), "名前")));
        // Latin is still Inter's, not the fallback's.
        assert!(ctx.fonts_mut(|f| f.has_glyphs(&font, "report")));
    }
}

/// Scroll bars that take their own room and can be seen whether or not
/// the pointer is over them, in the text colour: egui's own showed their
/// handle only under the pointer, in a button's fill, which the looks keep
/// clear, so a list or a tab with more below gave no sign of it.
pub fn steady_scroll() -> egui::style::ScrollStyle {
    let solid = egui::style::ScrollStyle::solid();
    egui::style::ScrollStyle {
        // Floating, so the opacities below take effect (egui draws a solid
        // bar at full strength, which read as a black or white slab) — but with its own room set aside, so it never lies
        // over the last row or column.
        floating: true,
        floating_width: solid.bar_width,
        floating_allocated_width: solid.bar_width,
        foreground_color: true,
        dormant_background_opacity: 0.08,
        dormant_handle_opacity: 0.3,
        active_background_opacity: 0.15,
        active_handle_opacity: 0.5,
        interact_background_opacity: 0.2,
        interact_handle_opacity: 0.7,
        ..solid
    }
}

#[cfg(test)]
mod scroll_tests {
    #[test]
    fn scroll_bars_are_seen_without_the_pointer_and_take_their_own_room() {
        let style = super::steady_scroll();
        assert!(
            !style.floating || style.floating_allocated_width >= style.bar_width,
            "a bar over the last row hides it"
        );
        assert!(style.dormant_handle_opacity < 0.5, "a solid slab");
        assert!(style.dormant_handle_opacity > 0.0, "seen only on hover");
        assert!(
            style.foreground_color,
            "drawn in a fill the looks keep clear"
        );
    }
}
