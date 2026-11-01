//! The window's icons. In the modern look they are Fluent UI System Icons,
//! Microsoft's open icons in the style of Fluent (MIT,
//! `fonts/LICENSE-FluentSystemIcons.txt`), as a font of the few dm draws;
//! the classic look keeps its own, drawn as strokes. Either way no image
//! files, and sharp at any scale.

use egui::{pos2, vec2, Color32, Painter, Rect, Shape, Stroke};

/// The Fluent icons dm draws, as the characters `fonts/FluentSystemIcons-dm.ttf`
/// holds them at (each `ic_fluent_<name>_20_regular`).
pub mod fluent {
    /// The Fluent add icon.
    pub const ADD: char = '\u{f109}';
    /// The Fluent play icon.
    pub const PLAY: char = '\u{f605}';
    /// The Fluent stop icon.
    pub const STOP: char = '\u{f72a}';
    /// The Fluent record stop icon.
    pub const RECORD_STOP: char = '\u{f75a}';
    /// The Fluent delete icon.
    pub const DELETE: char = '\u{f34c}';
    /// The Fluent broom icon.
    pub const BROOM: char = '\u{f201}';
    /// The Fluent settings icon.
    pub const SETTINGS: char = '\u{f6a9}';
    /// The Fluent calendar clock icon.
    pub const CALENDAR_CLOCK: char = '\u{f21d}';
    /// The Fluent play circle icon.
    pub const PLAY_CIRCLE: char = '\u{e994}';
    /// The Fluent pause circle icon.
    pub const PAUSE_CIRCLE: char = '\u{f6bc}';
    /// The Fluent globe icon.
    pub const GLOBE: char = '\u{f45a}';
    /// The Fluent apps list icon.
    pub const APPS_LIST: char = '\u{f8cb}';
    /// The Fluent video icon.
    pub const VIDEO: char = '\u{f84c}';
    /// The Fluent music icon.
    pub const MUSIC: char = '\u{e854}';
    /// The Fluent document icon.
    pub const DOCUMENT: char = '\u{f378}';
    /// The Fluent apps icon.
    pub const APPS: char = '\u{f133}';
    /// The Fluent folder zip icon.
    pub const FOLDER_ZIP: char = '\u{f435}';
    /// The Fluent image icon.
    pub const IMAGE: char = '\u{f488}';
    /// The Fluent folder icon.
    pub const FOLDER: char = '\u{f418}';
    /// The Fluent arrow download icon.
    pub const ARROW_DOWNLOAD: char = '\u{f150}';
    /// The Fluent checkmark circle icon.
    pub const CHECKMARK_CIRCLE: char = '\u{f298}';
    /// The Fluent list icon.
    pub const LIST: char = '\u{ecd9}';
    /// The Fluent arrow sync icon.
    pub const ARROW_SYNC: char = '\u{f190}';
    /// The Fluent chevron down icon.
    pub const CHEVRON_DOWN: char = '\u{f2a3}';
    /// The Fluent chevron up icon.
    pub const CHEVRON_UP: char = '\u{f2b6}';
    /// The Fluent chevron right icon.
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
        Icon::Folder => fluent::FOLDER,
        Icon::Details => fluent::LIST,
    }
}

/// A toolbar button's icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    /// The Add button.
    Add,
    /// The Resume button.
    Resume,
    /// The Stop button.
    Stop,
    /// The Stop All button.
    StopAll,
    /// The Delete button.
    Delete,
    /// The Delete Completed button.
    DeleteCompleted,
    /// The Options button.
    Options,
    /// The Scheduler button.
    Scheduler,
    /// The Start Queue button.
    StartQueue,
    /// The Stop Queue button.
    StopQueue,
    /// The Site files button.
    SiteFiles,
    /// Open the folder a file is in (a row's own button, not the toolbar's).
    Folder,
    /// A download's details window (a row's own button).
    Details,
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
        Icon::Folder => folder_outline(painter, rect, color),
        Icon::Details => {
            line((-0.7, -0.5), (0.7, -0.5));
            line((-0.7, 0.0), (0.7, 0.0));
            line((-0.7, 0.5), (0.7, 0.5));
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
    painter.extend(kind_shapes(kind, r, color));
}

/// The small picture on a file's sheet that tells its kind by shape, so the
/// kinds are told apart without relying on colour alone (a play mark, a note,
/// lines of text, a window, a zip, a hill and a sun). A file of no known kind
/// has a plain sheet.
pub fn kind_shapes(kind: Option<dm_core::Category>, sheet: Rect, color: Color32) -> Vec<Shape> {
    use dm_core::Category as K;
    // The white part of the sheet, above the coloured band.
    let area = Rect::from_min_max(
        pos2(sheet.min.x + 2.5, sheet.min.y + 2.5),
        pos2(sheet.max.x - 2.5, sheet.max.y - 6.5),
    );
    let c = area.center();
    let line = Stroke::new(1.0, color);
    match kind {
        Some(K::Videos) => vec![Shape::convex_polygon(
            vec![
                pos2(c.x - 1.5, area.top()),
                pos2(c.x - 1.5, area.bottom()),
                pos2(c.x + 2.0, c.y),
            ],
            color,
            Stroke::NONE,
        )],
        Some(K::Audio) => vec![
            Shape::circle_filled(pos2(c.x - 1.0, area.bottom() - 0.8), 1.2, color),
            Shape::line_segment(
                [
                    pos2(c.x + 0.2, area.bottom() - 0.8),
                    pos2(c.x + 0.2, area.top()),
                ],
                line,
            ),
        ],
        Some(K::Documents) => [area.top() + 0.5, c.y, area.bottom() - 0.5]
            .into_iter()
            .map(|y| Shape::line_segment([pos2(area.left(), y), pos2(area.right(), y)], line))
            .collect(),
        Some(K::Apps) => vec![
            Shape::rect_stroke(area, 0.5, line, egui::StrokeKind::Inside),
            Shape::line_segment(
                [
                    pos2(area.left(), area.top() + 1.8),
                    pos2(area.right(), area.top() + 1.8),
                ],
                line,
            ),
        ],
        Some(K::Archives) => [area.top(), area.top() + 2.5, area.top() + 5.0]
            .into_iter()
            .map(|y| {
                Shape::rect_filled(
                    Rect::from_min_size(pos2(c.x - 0.8, y), vec2(1.6, 1.4)),
                    0.0,
                    color,
                )
            })
            .collect(),
        Some(K::Pictures) => vec![
            Shape::convex_polygon(
                vec![
                    pos2(area.left(), area.bottom()),
                    pos2(c.x - 0.5, area.top() + 1.5),
                    pos2(area.right(), area.bottom()),
                ],
                color,
                Stroke::NONE,
            ),
            Shape::circle_filled(pos2(area.right() - 0.8, area.top() + 0.8), 0.9, color),
        ],
        _ => Vec::new(),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use dm_core::Category as K;

    const KINDS: [K; 6] = [
        K::Videos,
        K::Audio,
        K::Documents,
        K::Apps,
        K::Archives,
        K::Pictures,
    ];

    fn sheet() -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), vec2(11.0, 14.0))
    }

    #[test]
    fn each_kind_of_file_has_a_shape_of_its_own() {
        let drawn: Vec<String> = KINDS
            .iter()
            .map(|k| format!("{:?}", kind_shapes(Some(*k), sheet(), Color32::BLACK)))
            .collect();
        for (i, a) in drawn.iter().enumerate() {
            assert!(a.len() > 5, "{:?} draws nothing", KINDS[i]);
            for b in &drawn[i + 1..] {
                assert_ne!(a, b, "two kinds draw the same shape");
            }
        }
        assert!(kind_shapes(Some(K::Other), sheet(), Color32::BLACK).is_empty());
        assert!(kind_shapes(None, sheet(), Color32::BLACK).is_empty());
    }

    #[test]
    fn a_shape_stays_on_the_white_of_the_sheet() {
        let sheet = sheet();
        for kind in KINDS {
            for shape in kind_shapes(Some(kind), sheet, Color32::BLACK) {
                let at = shape.visual_bounding_rect();
                assert!(
                    at.min.x >= sheet.min.x + 1.0
                        && at.max.x <= sheet.max.x - 1.0
                        && at.min.y >= sheet.min.y + 1.0
                        && at.max.y <= sheet.max.y - 5.0,
                    "{kind:?} leaves the sheet: {at:?}"
                );
            }
        }
    }
}
