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

mod apply;
mod fonts;
mod scripts;

pub use apply::*;
pub use fonts::*;
pub use scripts::*;

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
