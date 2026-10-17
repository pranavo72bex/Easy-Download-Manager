//! dm's looks, as tokens: every colour, radius and shadow the window draws
//! comes from here.
//!
//! The default look is dm's modern one, light or dark as the computer is
//! set: neutral backgrounds, foregrounds and strokes, a blue brand colour,
//! 4 px control and 8 px card radii, and soft shadows. Its typeface is
//! Selawik, embedded under its open licence (SIL OFL 1.1,
//! `fonts/LICENSE-Selawik.txt`). A classic look stays as a choice in
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
            // The modern look, light.
            Theme::Light => Tokens {
                dark: false,
                modern: true,
                frame: hex(0xf5f5f5),
                pane: hex(0xfafafa),
                list: hex(0xffffff),
                header: hex(0xffffff),
                border: hex(0xe0e0e0),
                stroke: hex(0xd1d1d1),
                text: hex(0x242424),
                text2: hex(0x424242),
                weak: hex(0x616161),
                disabled: hex(0xbdbdbd),
                selection: hex(0xebebeb),
                selection_edge: hex(0xd1d1d1),
                selection_text: hex(0x242424),
                hover: hex(0xf5f5f5),
                pressed: hex(0xe0e0e0),
                accent: hex(0x0f6cbd),
                brand: hex(0x0f6cbd),
                brand_hover: hex(0x115ea3),
                brand_soft: hex(0xebf3fc),
                on_brand: Color32::WHITE,
                error: hex(0xc50f1f),
                success: hex(0x0e700e),
                warn: hex(0xbc4b09),
                folder: hex(0x616161),
                coloured_tools: false,
                radius: 4,
                card_radius: 8,
            },
            // The modern look, dark.
            Theme::Dark => Tokens {
                dark: true,
                modern: true,
                frame: hex(0x141414),
                pane: hex(0x1f1f1f),
                list: hex(0x292929),
                header: hex(0x292929),
                border: hex(0x3d3d3d),
                stroke: hex(0x666666),
                text: Color32::WHITE,
                text2: hex(0xd6d6d6),
                weak: hex(0xadadad),
                disabled: hex(0x5c5c5c),
                selection: hex(0x383838),
                selection_edge: hex(0x525252),
                selection_text: Color32::WHITE,
                hover: hex(0x3d3d3d),
                pressed: hex(0x2e2e2e),
                accent: hex(0x479ef5),
                brand: hex(0x115ea3),
                brand_hover: hex(0x0f6cbd),
                brand_soft: hex(0x082338),
                on_brand: Color32::WHITE,
                error: hex(0xdc626d),
                success: hex(0x54b054),
                warn: hex(0xfaa06b),
                folder: hex(0xadadad),
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
                border: hex(0xd4d4d4),
                stroke: hex(0xd4d4d4),
                text: hex(0x1a1a1a),
                text2: hex(0x1a1a1a),
                weak: hex(0x6d6d6d),
                disabled: hex(0xb4b4b4),
                selection: hex(0xcce8ff),
                selection_edge: hex(0x99d1ff),
                selection_text: hex(0x1a1a1a),
                hover: hex(0xe5f3ff),
                pressed: hex(0xcce8ff),
                accent: hex(0x0078d7),
                brand: hex(0x0078d7),
                brand_hover: hex(0x005a9e),
                brand_soft: hex(0xe5f3ff),
                on_brand: Color32::WHITE,
                error: hex(0xc42b1c),
                success: hex(0x107c10),
                warn: hex(0xb25e09),
                folder: hex(0xf2c24b),
                coloured_tools: true,
                radius: 2,
                card_radius: 4,
            },
        }
    }
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
    /// Hairlines and the card's edge.
    pub border: Color32,
    /// Control outlines.
    pub stroke: Color32,
    pub text: Color32,
    pub text2: Color32,
    pub weak: Color32,
    pub disabled: Color32,
    pub selection: Color32,
    pub selection_edge: Color32,
    pub selection_text: Color32,
    pub hover: Color32,
    pub pressed: Color32,
    pub accent: Color32,
    /// The primary button and the selection indicator.
    pub brand: Color32,
    pub brand_hover: Color32,
    pub brand_soft: Color32,
    pub on_brand: Color32,
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

/// Whether Selawik's semibold is in force: set on an earlier frame, which
/// is when egui starts using fonts given to `set_fonts`. Kept as a frame
/// number rather than asked of egui, which panics if asked before its
/// first frame.
pub fn fonts_ready(ctx: &Context) -> bool {
    ctx.data(|d| d.get_temp::<u64>(egui::Id::unique("dm-fonts-pass")))
        .is_some_and(|set| ctx.cumulative_pass_nr() > set)
}

/// Embeds Selawik once, an openly licensed typeface (SIL OFL).
fn fonts(ctx: &Context) {
    let id = egui::Id::unique("dm-fonts-pass");
    if ctx.data(|d| d.get_temp::<u64>(id)).is_some() {
        return;
    }
    ctx.set_fonts(definitions(&scripts_asked(ctx)));
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.insert_temp(id, pass));
}

/// dm's fonts: Selawik first, egui's own after it, and the computer's
/// fonts for `scripts` last of all, so each only draws what the others
/// cannot.
fn definitions(scripts: &[Script]) -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "selawik".into(),
        egui::FontData::from_static(include_bytes!("../fonts/Selawik-Regular.ttf")).into(),
    );
    fonts.font_data.insert(
        "selawik-semibold".into(),
        egui::FontData::from_static(include_bytes!("../fonts/Selawik-Semibold.ttf")).into(),
    );
    if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
        family.insert(0, "selawik".into());
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
    let mut semibold = vec!["selawik-semibold".to_string()];
    semibold.extend(
        fonts
            .families
            .get(&FontFamily::Proportional)
            .cloned()
            .unwrap_or_default(),
    );
    fonts.families.insert(semibold_family(), semibold);
    fonts
}

/* ------------------------------------------- the computer's own fonts */

/// Writing systems Selawik and egui's fonts have no letters for, which a
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
    #[cfg(target_os = "linux")]
    if let Some(found) = fontconfig(script) {
        return Some(found);
    }
    script
        .known_files()
        .iter()
        .map(std::path::PathBuf::from)
        .find(|p| p.is_file())
        .map(|p| (p, 0))
}

/// Asks fontconfig for a sans font that covers `script`'s language, and
/// takes it only if it does: `fc-match` always answers with something.
#[cfg(target_os = "linux")]
fn fontconfig(script: Script) -> Option<(std::path::PathBuf, u32)> {
    let language = script.language();
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
            let bytes = std::fs::read(path).ok()?;
            Some(egui::FontData::from_blob(std::sync::Arc::new(bytes), index))
        })
        .clone()
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
    fonts(ctx);
    let t = theme.tokens();
    let mut v = if t.dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    v.panel_fill = t.frame;
    v.window_fill = t.list;
    v.extreme_bg_color = t.list;
    v.faint_bg_color = t.header;
    v.code_bg_color = t.pane;
    v.override_text_color = Some(t.text);
    v.selection.bg_fill = t.selection;
    v.selection.stroke = Stroke::new(1.0, t.selection_text);
    v.hyperlink_color = t.accent;
    v.error_fg_color = t.error;
    v.warn_fg_color = t.warn;
    v.window_stroke = Stroke::new(1.0, t.border);
    let radius = CornerRadius::same(t.radius);
    v.window_corner_radius = CornerRadius::same(t.card_radius);
    v.menu_corner_radius = CornerRadius::same(t.card_radius);
    // A deeper shadow for dialogs, a lighter one for menus.
    let alpha = if t.dark { 72 } else { 36 };
    v.window_shadow = Shadow {
        offset: [0, 8],
        blur: 16,
        spread: 0,
        color: Color32::from_black_alpha(alpha),
    };
    v.popup_shadow = Shadow {
        offset: [0, 4],
        blur: 8,
        spread: 0,
        color: Color32::from_black_alpha(alpha),
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
    // The secondary button: the card's colour with a hairline.
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, t.border);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, t.text);
    v.widgets.inactive.bg_fill = t.list;
    v.widgets.inactive.weak_bg_fill = t.list;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, t.stroke);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, t.text);
    v.widgets.hovered.bg_fill = t.hover;
    v.widgets.hovered.weak_bg_fill = t.hover;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, t.stroke);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, t.text);
    v.widgets.active.bg_fill = t.pressed;
    v.widgets.active.weak_bg_fill = t.pressed;
    v.widgets.active.bg_stroke = Stroke::new(1.0, t.stroke);
    v.widgets.active.fg_stroke = Stroke::new(1.0, t.text);
    v.widgets.open.bg_fill = t.selection;
    v.widgets.open.weak_bg_fill = t.selection;
    v.widgets.open.fg_stroke = Stroke::new(1.0, t.text);

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
        // Type sizes: body 14, caption 12, and 18 for titles.
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
            (TextStyle::Heading, FontId::new(18.0, heading.clone())),
            (
                TextStyle::Monospace,
                FontId::new(13.0, FontFamily::Monospace),
            ),
        ]
        .into();
        // From egui's own each time: Light's roomier spacing stayed when a
        // running dm went to Classic, which only leaves it alone.
        style.spacing = egui::style::Spacing::default();
        if t.modern {
            style.spacing.button_padding = egui::vec2(12.0, 5.0);
            style.spacing.item_spacing = egui::vec2(8.0, 6.0);
            style.spacing.interact_size.y = 28.0;
            style.spacing.menu_margin = egui::Margin::same(4);
            style.spacing.window_margin = egui::Margin::same(16);
        }
    });
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique("dm-tokens"), t));
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

    #[test]
    fn the_modern_palette_is_dms_own() {
        let light = Theme::Light.tokens();
        assert_eq!(light.brand, hex(0x0f6cbd), "dm's brand blue");
        assert_eq!(light.text, hex(0x242424), "from the palette");
        assert_eq!((light.radius, light.card_radius), (4, 8));
        let dark = Theme::Dark.tokens();
        assert_eq!(dark.accent, hex(0x479ef5), "brandForeground1, dark");
        assert_eq!(dark.list, hex(0x292929), "the list, dark");
        // Text must stand out from the card it is drawn on, in every look.
        for theme in [Theme::Light, Theme::Dark, Theme::Classic] {
            let t = theme.tokens();
            let light = |c: Color32| c.r() as i32 + c.g() as i32 + c.b() as i32;
            assert!((light(t.text) - light(t.list)).abs() > 400, "{theme:?}");
            assert!(
                (light(t.on_brand) - light(t.brand)).abs() > 300,
                "{theme:?}"
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
        assert_eq!(ctx.global_style().spacing.interact_size.y, 28.0);
        apply(&ctx, Theme::Classic);
        let fresh = egui::style::Spacing::default();
        let now = ctx.global_style().spacing.clone();
        assert_eq!(now.button_padding, fresh.button_padding);
        assert_eq!(now.item_spacing, fresh.item_spacing);
        assert_eq!(now.interact_size, fresh.interact_size);
        assert_eq!(now.window_margin, fresh.window_margin);
    }

    /// Gap 116: a name in Chinese and Japanese is drawn with the computer's
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
        // Latin is still Selawik's, not the fallback's.
        assert!(ctx.fonts_mut(|f| f.has_glyphs(&font, "report")));
    }
}
