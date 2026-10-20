//! The window's icons. In the modern look they are Fluent UI System Icons,
//! Microsoft's open icons in the style of Fluent (MIT,
//! `fonts/LICENSE-FluentSystemIcons.txt`), as a font of the few dm draws;
//! the classic look keeps its own, drawn as strokes. Either way no image
//! files, and sharp at any scale.

use egui::{pos2, vec2, Color32, Painter, Rect, Shape, Stroke};

/// The Fluent icons dm draws, as the characters `fonts/FluentSystemIcons-dm.ttf`
/// holds them at (each `ic_fluent_<name>_20_regular`).
pub mod fluent {
    pub const ADD: char = '\u{f109}';
    pub const PLAY: char = '\u{f605}';
    pub const STOP: char = '\u{f72a}';
    pub const RECORD_STOP: char = '\u{f75a}';
    pub const DELETE: char = '\u{f34c}';
    pub const BROOM: char = '\u{f201}';
    pub const SETTINGS: char = '\u{f6a9}';
    pub const CALENDAR_CLOCK: char = '\u{f21d}';
    pub const PLAY_CIRCLE: char = '\u{e994}';
    pub const PAUSE_CIRCLE: char = '\u{f6bc}';
    pub const GLOBE: char = '\u{f45a}';
    pub const APPS_LIST: char = '\u{f8cb}';
    pub const VIDEO: char = '\u{f84c}';
    pub const MUSIC: char = '\u{e854}';
    pub const DOCUMENT: char = '\u{f378}';
    pub const APPS: char = '\u{f133}';
    pub const FOLDER_ZIP: char = '\u{f435}';
    pub const IMAGE: char = '\u{f488}';
    pub const FOLDER: char = '\u{f418}';
    pub const ARROW_DOWNLOAD: char = '\u{f150}';
    pub const CHECKMARK_CIRCLE: char = '\u{f298}';
    pub const LIST: char = '\u{ecd9}';
    pub const ARROW_SYNC: char = '\u{f190}';
    pub const CHEVRON_DOWN: char = '\u{f2a3}';
    pub const CHEVRON_UP: char = '\u{f2b6}';
    pub const CHEVRON_RIGHT: char = '\u{f2b0}';
}

/// The font family the Fluent icons are drawn from.
pub fn family() -> egui::FontFamily {
    egui::FontFamily::Name("icons".into())
}

/// Draws a Fluent icon centred in `rect`, as big as its shorter side. False
/// where the icons are not loaded yet (the first frame) or the look is the
/// classic one, for the caller to draw its own.
pub fn glyph(painter: &Painter, rect: Rect, icon: char, color: Color32) -> bool {
    let ctx = painter.ctx();
    if !crate::theme::fonts_ready(ctx) || !crate::theme::Tokens::get(ctx).modern {
        return false;
    }
    let size = rect.width().min(rect.height());
    let galley = painter.layout_no_wrap(icon.to_string(), egui::FontId::new(size, family()), color);
    // By the glyph's own box, which a row's height does not centre.
    let at = rect.center() - galley.size() / 2.0;
    painter.galley(at, galley, color);
    true
}

/// The Fluent icon for a toolbar command.
fn fluent_of(icon: Icon) -> char {
    match icon {
        Icon::Add => fluent::ADD,
        Icon::Resume => fluent::PLAY,
        Icon::Stop => fluent::STOP,
        Icon::StopAll => fluent::RECORD_STOP,
        Icon::Delete => fluent::DELETE,
        Icon::DeleteCompleted => fluent::BROOM,
        Icon::Options => fluent::SETTINGS,
        Icon::Scheduler => fluent::CALENDAR_CLOCK,
        Icon::StartQueue => fluent::PLAY_CIRCLE,
        Icon::StopQueue => fluent::PAUSE_CIRCLE,
        Icon::SiteFiles => fluent::GLOBE,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Add,
    Resume,
    Stop,
    StopAll,
    Delete,
    DeleteCompleted,
    Options,
    Scheduler,
    StartQueue,
    StopQueue,
    SiteFiles,
}

/// Draws `icon` inside `rect` in `color`.
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    if glyph(painter, rect, fluent_of(icon), color) {
        return;
    }
    let s = rect.width().min(rect.height());
    let c = rect.center();
    let stroke = Stroke::new((s / 12.0).max(1.5), color);
    let at = |x: f32, y: f32| pos2(c.x + x * s / 2.0, c.y + y * s / 2.0);
    let line =
        |a: (f32, f32), b: (f32, f32)| painter.line_segment([at(a.0, a.1), at(b.0, b.1)], stroke);
    let square = |half: f32, dx: f32, dy: f32| {
        painter.rect_stroke(
            Rect::from_center_size(at(dx, dy), vec2(half * s, half * s)),
            1.0,
            stroke,
            egui::StrokeKind::Middle,
        )
    };
    let triangle = |scale: f32| {
        painter.add(Shape::convex_polygon(
            vec![
                at(-0.45 * scale, -0.6 * scale),
                at(0.6 * scale, 0.0),
                at(-0.45 * scale, 0.6 * scale),
            ],
            // An outline, as every other action's icon is (a design
            // review found play and stop filled beside outlines).
            Color32::TRANSPARENT,
            stroke,
        ))
    };
    match icon {
        Icon::Add => {
            line((0.0, -0.75), (0.0, 0.75));
            line((-0.75, 0.0), (0.75, 0.0));
        }
        Icon::Resume => {
            triangle(1.0);
        }
        Icon::Stop => {
            square(0.6, 0.0, 0.0);
        }
        Icon::StopAll => {
            square(0.55, -0.2, -0.2);
            square(0.55, 0.2, 0.2);
        }
        Icon::Delete => {
            line((-0.6, -0.6), (0.6, 0.6));
            line((-0.6, 0.6), (0.6, -0.6));
        }
        Icon::DeleteCompleted => {
            line((-0.8, 0.0), (-0.35, 0.45));
            line((-0.35, 0.45), (0.2, -0.4));
            line((0.35, 0.1), (0.85, 0.6));
            line((0.35, 0.6), (0.85, 0.1));
        }
        Icon::Options => {
            painter.circle_stroke(c, s * 0.22, stroke);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::FRAC_PI_4;
                let (x, y) = (a.cos(), a.sin());
                line((x * 0.5, y * 0.5), (x * 0.78, y * 0.78));
            }
        }
        Icon::Scheduler => {
            painter.circle_stroke(c, s * 0.42, stroke);
            line((0.0, 0.0), (0.0, -0.55));
            line((0.0, 0.0), (0.4, 0.2));
        }
        Icon::StartQueue => {
            painter.circle_stroke(c, s * 0.45, stroke);
            triangle(0.55);
        }
        Icon::StopQueue => {
            painter.circle_stroke(c, s * 0.45, stroke);
            square(0.34, 0.0, 0.0);
        }
        Icon::SiteFiles => {
            painter.circle_stroke(c, s * 0.42, stroke);
            line((-0.84, 0.0), (0.84, 0.0));
            painter.add(Shape::ellipse_stroke(c, vec2(s * 0.18, s * 0.42), stroke));
        }
    }
}

/// A folder, as the category tree draws them.
pub fn folder(painter: &Painter, rect: Rect, color: Color32) {
    let r = Rect::from_center_size(rect.center(), vec2(14.0, 10.0));
    let tab = Rect::from_min_size(r.min - vec2(0.0, 2.0), vec2(6.0, 3.0));
    let edge = Color32::from_rgba_unmultiplied(0, 0, 0, 60);
    painter.rect_filled(tab, 1.0, color);
    painter.rect_filled(r, 1.5, color);
    painter.rect_stroke(r, 1.5, Stroke::new(1.0, edge), egui::StrokeKind::Inside);
}

/// A sheet with a folded corner in the colour of the file's kind, before a
/// row's name.
pub fn file(painter: &Painter, rect: Rect, kind: Option<dm_core::Category>) {
    use dm_core::Category as K;
    let color = match kind {
        Some(K::Videos) => Color32::from_rgb(0x8e, 0x44, 0xad),
        Some(K::Audio) => Color32::from_rgb(0xe6, 0x7e, 0x22),
        Some(K::Documents) => Color32::from_rgb(0x2b, 0x6c, 0xc4),
        Some(K::Apps) => Color32::from_rgb(0x3d, 0x8b, 0x37),
        Some(K::Archives) => Color32::from_rgb(0xb7, 0x93, 0x1f),
        Some(K::Pictures) => Color32::from_rgb(0xc0, 0x39, 0x5a),
        _ => Color32::from_rgb(0x7f, 0x8c, 0x8d),
    };
    let r = Rect::from_center_size(rect.center(), vec2(11.0, 14.0));
    painter.rect_filled(r, 1.5, Color32::WHITE);
    painter.rect_stroke(r, 1.5, Stroke::new(1.0, color), egui::StrokeKind::Inside);
    let band = Rect::from_min_max(pos2(r.min.x, r.max.y - 5.0), r.max);
    painter.rect_filled(band, 1.0, color);
}

/// A folder drawn in outline, as the modern look's icons are.
pub fn folder_outline(painter: &Painter, rect: Rect, color: Color32) {
    let r = Rect::from_center_size(rect.center() + vec2(0.0, 1.0), vec2(14.0, 10.0));
    let stroke = Stroke::new(1.3, color);
    let tab = [
        pos2(r.left(), r.top()),
        pos2(r.left(), r.top() - 2.0),
        pos2(r.left() + 5.0, r.top() - 2.0),
        pos2(r.left() + 6.5, r.top()),
    ];
    painter.add(Shape::line(tab.to_vec(), stroke));
    painter.rect_stroke(r, 1.5, stroke, egui::StrokeKind::Inside);
}
