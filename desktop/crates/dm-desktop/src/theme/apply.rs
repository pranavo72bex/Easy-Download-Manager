//! Applying a theme and its accent to the window.

use super::*;

pub(super) fn semibold_family() -> FontFamily {
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
    // High Contrast is the system's colours and no other: not the accent.
    let high_contrast = matches!(theme, Theme::HighContrast(_));
    let mut t = match &accent {
        Some(palette) if !high_contrast => crate::accent::tokens(theme.tokens(), palette),
        _ => theme.tokens(),
    };
    // Under Mica the frame is clear and the backdrop is its colour; the
    // list, dialogs and fields stay solid to be read on.
    // (Not in High Contrast, which has no material.)
    if crate::chrome::mica() && !high_contrast {
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
    // egui outlines a focused text field in this stroke, which is also the
    // colour selected words are drawn in (white in a dialog's fields). The
    // modern look draws its own focus frame and accent line
    // (`underline_fields`), and the outline egui added showed white at the
    // rounded corners in every field, so there it has no width.
    let outline = if t.modern { 0.0 } else { 1.0 };
    v.selection.stroke = Stroke::new(outline, t.selection_text);
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
    // Fluent's elevation, as the one shadow egui draws: a menu at 32 (the
    // tooltip shares egui's popup shadow), and a dialog at the deepest of
    // 128, 64 and 28 its window has room for (`dialog_shadow_blur`): egui
    // counts a window's shadow in its size, and at 200% a dialog with a deep
    // one no longer fitted the window. This one is the floor.
    let (deep, soft) = if t.dark { (71, 71) } else { (61, 36) };
    v.window_shadow = Shadow {
        offset: [0, 14],
        blur: 28,
        spread: 0,
        color: Color32::from_black_alpha(deep),
    };
    v.popup_shadow = Shadow {
        offset: [0, 16],
        blur: 32,
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
            // WinUI's standard density: controls 32 tall (the Fluent 2
            // standard control height), buttons padded 11 by 5; 8 between
            // controls, a dialog padded 24, a check box's box 20 with 8
            // before its words. A menu's items are 32 with no gap between.
            style.spacing.button_padding = egui::vec2(11.0, 5.0);
            style.spacing.item_spacing = egui::vec2(8.0, 8.0);
            style.spacing.interact_size.y = 32.0;
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
