//! The toolbar's icons, drawn as strokes rather than shipped as pictures:
//! no image files, and sharp at any scale.

use egui::{pos2, vec2, Color32, Painter, Rect, Shape, Stroke};

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
            color,
            Stroke::NONE,
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
            painter.rect_filled(
                Rect::from_center_size(c, vec2(s * 0.6, s * 0.6)),
                1.0,
                color,
            );
        }
        Icon::StopAll => {
            square(0.55, -0.2, -0.2);
            painter.rect_filled(
                Rect::from_center_size(at(0.2, 0.2), vec2(s * 0.55, s * 0.55)),
                1.0,
                color,
            );
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
            painter.rect_filled(
                Rect::from_center_size(c, vec2(s * 0.34, s * 0.34)),
                1.0,
                color,
            );
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
