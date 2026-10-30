//! Line icons drawn with the painter on a 24-unit grid, in the thin outline style of the
//! original's toolbar. Our own drawings: no glyph or image comes from the original.

use eframe::egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2};
use std::f32::consts::{PI, TAU};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Menu,
    Print3D,
    Save,
    SaveAs,
    Undo,
    Redo,
    Back,
    Check,
    Close,
    Minimize,
    Maximize,
    ChevronLeft,
    ChevronRight,
    ChevronDown,
    ChevronUp,
    // Insert
    Add,
    Custom,
    Cube,
    Cylinder,
    Pyramid,
    Cone,
    Sphere,
    Hexagon,
    Wedge,
    Torus,
    Tetrahedron,
    // Object
    Duplicate,
    Copy,
    Cut,
    Paste,
    Delete,
    Settle,
    Mirror,
    Measure,
    // Edit
    Simplify,
    Split,
    Smooth,
    Emboss,
    ExtrudeDown,
    Merge,
    Intersect,
    Subtract,
    Hollow,
    // Paint
    Material,
    Color,
    Texture,
    // View
    Eye,
    Shading,
    Shadows,
    Colors,
    Reflections,
    Smoothing,
    Wireframe,
    Grid,
    XRay,
    // Help
    Help,
    Info,
    Guide,
    // Selection
    Group,
    Ungroup,
    SelectAll,
    SelectNone,
    Invert,
    SpecialSelect,
    // Menu
    NewScene,
    Open,
    Cart,
    Printer,
    AboutModel,
    PrinterStatus,
    Feedback,
    Assistant,
    Settings,
    Smile,
    // Transform
    Move,
    Rotate,
    Scale,
    Lock,
    Unlock,
    /// Three axes from one corner: reset orientation.
    Axes,
    /// A page with an arrow up at its lower right: work in progress on a file.
    FileUp,
    Wrench,
    KeepTop,
    Warning,
    ZoomIn,
    ZoomOut,
    Pencil,
    More,
}

/// Draws `icon` centred in `rect` (a square works best).
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let s = rect.width().min(rect.height()) / 24.0;
    let o = rect.center() - Vec2::splat(12.0 * s);
    let pen = Pen {
        painter,
        o,
        s,
        stroke: Stroke::new((1.3 * s).max(1.0), color),
        color,
    };
    pen.draw(icon);
}

struct Pen<'a> {
    painter: &'a Painter,
    o: Pos2,
    s: f32,
    stroke: Stroke,
    color: Color32,
}

impl Pen<'_> {
    fn p(&self, x: f32, y: f32) -> Pos2 {
        self.o + Vec2::new(x, y) * self.s
    }
    fn line(&self, pts: &[(f32, f32)]) {
        let pts: Vec<Pos2> = pts.iter().map(|&(x, y)| self.p(x, y)).collect();
        self.painter.add(Shape::line(pts, self.stroke));
    }
    fn closed(&self, pts: &[(f32, f32)]) {
        let pts: Vec<Pos2> = pts.iter().map(|&(x, y)| self.p(x, y)).collect();
        self.painter.add(Shape::closed_line(pts, self.stroke));
    }
    fn fill(&self, pts: &[(f32, f32)]) {
        let pts: Vec<Pos2> = pts.iter().map(|&(x, y)| self.p(x, y)).collect();
        self.painter
            .add(Shape::convex_polygon(pts, self.color, Stroke::NONE));
    }
    fn rect(&self, x0: f32, y0: f32, x1: f32, y1: f32) {
        self.closed(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)]);
    }
    fn circle(&self, x: f32, y: f32, r: f32) {
        self.painter
            .circle_stroke(self.p(x, y), r * self.s, self.stroke);
    }
    fn dot(&self, x: f32, y: f32, r: f32) {
        self.painter
            .circle_filled(self.p(x, y), r * self.s, self.color);
    }
    /// Arc around (x, y) from angle a0 to a1 (radians, y down).
    fn arc(&self, x: f32, y: f32, r: f32, a0: f32, a1: f32) {
        let n = ((a1 - a0).abs() / 0.2).ceil().max(2.0) as usize;
        let pts: Vec<(f32, f32)> = (0..=n)
            .map(|i| {
                let a = a0 + (a1 - a0) * i as f32 / n as f32;
                (x + r * a.cos(), y + r * a.sin())
            })
            .collect();
        self.line(&pts);
    }
    fn ellipse(&self, x: f32, y: f32, rx: f32, ry: f32, a0: f32, a1: f32) {
        let n = 24;
        let pts: Vec<(f32, f32)> = (0..=n)
            .map(|i| {
                let a = a0 + (a1 - a0) * i as f32 / n as f32;
                (x + rx * a.cos(), y + ry * a.sin())
            })
            .collect();
        self.line(&pts);
    }
    fn dashed_rect(&self, x0: f32, y0: f32, x1: f32, y1: f32) {
        for (a, b) in [
            ((x0, y0), (x1, y0)),
            ((x1, y0), (x1, y1)),
            ((x1, y1), (x0, y1)),
            ((x0, y1), (x0, y0)),
        ] {
            let len = (b.0 - a.0).hypot(b.1 - a.1);
            let n = (len / 3.0).floor().max(1.0) as usize;
            for i in (0..n).step_by(2) {
                let t0 = i as f32 / n as f32;
                let t1 = ((i + 1) as f32 / n as f32).min(1.0);
                self.line(&[
                    (a.0 + (b.0 - a.0) * t0, a.1 + (b.1 - a.1) * t0),
                    (a.0 + (b.0 - a.0) * t1, a.1 + (b.1 - a.1) * t1),
                ]);
            }
        }
    }
    fn arrow_head(&self, x: f32, y: f32, dx: f32, dy: f32) {
        // Two short strokes back from the tip (x, y) along −(dx, dy).
        let (nx, ny) = (-dy, dx);
        self.line(&[(x - dx * 3.0 + nx * 3.0, y - dy * 3.0 + ny * 3.0), (x, y)]);
        self.line(&[(x - dx * 3.0 - nx * 3.0, y - dy * 3.0 - ny * 3.0), (x, y)]);
    }

    fn draw(&self, icon: Icon) {
        use Icon::*;
        match icon {
            Menu => {
                for y in [7.0, 12.0, 17.0] {
                    self.line(&[(3.0, y), (21.0, y)]);
                }
            }
            Print3D => {
                self.rect(3.0, 4.0, 21.0, 20.0);
                self.line(&[(3.0, 8.0), (21.0, 8.0)]);
                self.closed(&[(12.0, 10.0), (17.0, 18.0), (7.0, 18.0)]);
            }
            Save => {
                self.closed(&[
                    (4.0, 4.0),
                    (18.0, 4.0),
                    (20.0, 6.0),
                    (20.0, 20.0),
                    (4.0, 20.0),
                ]);
                self.rect(8.0, 4.0, 16.0, 9.0);
                self.rect(7.0, 13.0, 17.0, 20.0);
            }
            SaveAs => {
                self.closed(&[(4.0, 4.0), (16.0, 4.0), (18.0, 6.0), (18.0, 12.0)]);
                self.line(&[(4.0, 4.0), (4.0, 20.0), (11.0, 20.0)]);
                self.rect(7.0, 4.0, 13.0, 8.0);
                self.line(&[
                    (13.0, 21.0),
                    (21.0, 13.0),
                    (19.0, 11.0),
                    (11.0, 19.0),
                    (13.0, 21.0),
                ]);
            }
            Undo => {
                self.arc(13.0, 13.0, 7.0, -PI * 0.5, PI * 0.55);
                self.line(&[(13.0, 6.0), (6.0, 6.0)]);
                self.line(&[(9.0, 3.0), (6.0, 6.0), (9.0, 9.0)]);
            }
            Redo => {
                self.arc(11.0, 13.0, 7.0, PI * 0.45, PI * 1.5);
                self.line(&[(11.0, 6.0), (18.0, 6.0)]);
                self.line(&[(15.0, 3.0), (18.0, 6.0), (15.0, 9.0)]);
            }
            Back => {
                self.line(&[(4.0, 12.0), (20.0, 12.0)]);
                self.line(&[(11.0, 5.0), (4.0, 12.0), (11.0, 19.0)]);
            }
            Check => self.line(&[(4.0, 12.0), (9.0, 17.0), (20.0, 6.0)]),
            Close => {
                self.line(&[(5.0, 5.0), (19.0, 19.0)]);
                self.line(&[(19.0, 5.0), (5.0, 19.0)]);
            }
            Minimize => self.line(&[(6.0, 12.0), (18.0, 12.0)]),
            Maximize => self.rect(7.0, 7.0, 17.0, 17.0),
            ChevronLeft => self.line(&[(15.0, 4.0), (8.0, 12.0), (15.0, 20.0)]),
            ChevronRight => self.line(&[(9.0, 4.0), (16.0, 12.0), (9.0, 20.0)]),
            ChevronDown => self.line(&[(5.0, 9.0), (12.0, 16.0), (19.0, 9.0)]),
            ChevronUp => self.line(&[(5.0, 15.0), (12.0, 8.0), (19.0, 15.0)]),
            Add => {
                self.line(&[(12.0, 3.0), (12.0, 21.0)]);
                self.line(&[(3.0, 12.0), (21.0, 12.0)]);
            }
            Custom => {
                self.circle(8.0, 15.0, 4.5);
                self.closed(&[(12.0, 4.0), (17.0, 12.0), (7.0, 12.0)]);
                self.rect(13.0, 12.0, 21.0, 19.0);
            }
            Cube => {
                self.closed(&[
                    (12.0, 3.0),
                    (20.0, 7.0),
                    (20.0, 17.0),
                    (12.0, 21.0),
                    (4.0, 17.0),
                    (4.0, 7.0),
                ]);
                self.line(&[(4.0, 7.0), (12.0, 11.0), (20.0, 7.0)]);
                self.line(&[(12.0, 11.0), (12.0, 21.0)]);
            }
            Cylinder => {
                self.ellipse(12.0, 6.0, 7.0, 2.5, 0.0, TAU);
                self.line(&[(5.0, 6.0), (5.0, 18.0)]);
                self.line(&[(19.0, 6.0), (19.0, 18.0)]);
                self.ellipse(12.0, 18.0, 7.0, 2.5, 0.0, PI);
            }
            Pyramid => {
                self.closed(&[(12.0, 3.0), (21.0, 19.0), (3.0, 19.0)]);
                self.line(&[(12.0, 3.0), (14.0, 19.0)]);
            }
            Cone => {
                self.line(&[(7.0, 17.0), (12.0, 3.0), (17.0, 17.0)]);
                self.ellipse(12.0, 17.0, 5.0, 2.5, 0.0, TAU);
            }
            Sphere => self.circle(12.0, 12.0, 8.5),
            Hexagon => {
                self.ellipse(12.0, 9.0, 8.0, 3.0, 0.0, TAU);
                self.line(&[(4.0, 9.0), (4.0, 14.0)]);
                self.line(&[(20.0, 9.0), (20.0, 14.0)]);
                self.ellipse(12.0, 14.0, 8.0, 3.0, 0.0, PI);
            }
            Wedge => {
                self.closed(&[(12.0, 4.0), (20.0, 19.0), (4.0, 19.0)]);
                self.line(&[(12.0, 4.0), (12.0, 13.0), (4.0, 19.0)]);
                self.line(&[(12.0, 13.0), (20.0, 19.0)]);
            }
            Torus => {
                self.ellipse(12.0, 12.0, 9.0, 4.0, 0.0, TAU);
                self.ellipse(12.0, 11.5, 4.0, 1.3, 0.0, TAU);
            }
            Tetrahedron => {
                self.closed(&[(12.0, 3.0), (21.0, 20.0), (3.0, 20.0)]);
                self.line(&[(12.0, 3.0), (12.0, 20.0)]);
            }
            Duplicate => {
                self.closed(&[(3.0, 19.0), (9.0, 8.0), (15.0, 19.0)]);
                self.line(&[(9.0, 8.0), (11.0, 5.0), (21.0, 19.0), (15.0, 19.0)]);
            }
            Copy => {
                self.rect(8.0, 7.0, 19.0, 21.0);
                self.line(&[(5.0, 17.0), (5.0, 3.0), (15.0, 3.0)]);
            }
            Cut => {
                self.circle(7.0, 18.0, 3.0);
                self.circle(17.0, 18.0, 3.0);
                self.line(&[(9.0, 16.0), (16.0, 3.0)]);
                self.line(&[(15.0, 16.0), (8.0, 3.0)]);
            }
            Paste => {
                self.rect(5.0, 5.0, 19.0, 21.0);
                self.rect(9.0, 3.0, 15.0, 7.0);
                self.rect(11.0, 11.0, 20.0, 21.0);
            }
            Delete => {
                self.line(&[(4.0, 6.0), (20.0, 6.0)]);
                self.line(&[(9.0, 6.0), (9.0, 3.0), (15.0, 3.0), (15.0, 6.0)]);
                self.closed(&[(6.0, 6.0), (7.0, 21.0), (17.0, 21.0), (18.0, 6.0)]);
                for x in [10.0, 12.0, 14.0] {
                    self.line(&[(x, 9.0), (x, 18.0)]);
                }
            }
            Settle => {
                self.fill(&[(7.0, 5.0), (19.0, 3.0), (17.0, 15.0), (5.0, 17.0)]);
                self.line(&[(3.0, 21.0), (21.0, 21.0)]);
            }
            Mirror => {
                self.line(&[(3.0, 8.0), (20.0, 8.0)]);
                self.arrow_head(20.0, 8.0, 1.0, 0.0);
                self.line(&[(21.0, 16.0), (4.0, 16.0)]);
                self.arrow_head(4.0, 16.0, -1.0, 0.0);
            }
            Measure => {
                self.line(&[(2.0, 12.0), (22.0, 12.0)]);
                self.line(&[(4.0, 8.0), (4.0, 16.0)]);
                self.line(&[(20.0, 8.0), (20.0, 16.0)]);
                for x in [8.0, 12.0, 16.0] {
                    self.line(&[(x, 9.0), (x, 12.0)]);
                }
            }
            Simplify => {
                for y in [5.0, 12.0, 19.0] {
                    self.line(&[(3.0, y), (21.0, y)]);
                }
                self.line(&[(12.0, 7.0), (12.0, 17.0)]);
                self.arrow_head(12.0, 10.0, 0.0, 1.0);
            }
            Split => {
                self.closed(&[(12.0, 3.0), (17.0, 13.0), (7.0, 13.0)]);
                self.line(&[(3.0, 16.0), (21.0, 16.0)]);
                self.fill(&[(6.0, 18.0), (18.0, 18.0), (19.0, 21.0), (5.0, 21.0)]);
            }
            Smooth => {
                self.line(&[(3.0, 4.0), (21.0, 4.0)]);
                self.line(&[(3.0, 20.0), (21.0, 20.0)]);
                self.line(&[(12.0, 7.0), (12.0, 17.0)]);
                self.arrow_head(12.0, 7.0, 0.0, -1.0);
                self.arrow_head(12.0, 17.0, 0.0, 1.0);
            }
            Emboss => {
                let pts: Vec<(f32, f32)> = (0..10)
                    .map(|i| {
                        let r = if i % 2 == 0 { 9.5 } else { 4.0 };
                        let a = -PI / 2.0 + i as f32 * PI / 5.0;
                        (12.0 + r * a.cos(), 12.5 + r * a.sin())
                    })
                    .collect();
                self.closed(&pts);
            }
            ExtrudeDown => {
                self.rect(4.0, 4.0, 20.0, 20.0);
                self.fill(&[(4.0, 4.0), (20.0, 4.0), (20.0, 11.0), (4.0, 11.0)]);
            }
            Merge => {
                self.rect(3.0, 3.0, 14.0, 14.0);
                self.rect(10.0, 10.0, 21.0, 21.0);
                self.fill(&[(10.0, 10.0), (14.0, 10.0), (14.0, 14.0), (10.0, 14.0)]);
            }
            Intersect => {
                self.dashed_rect(3.0, 3.0, 15.0, 15.0);
                self.dashed_rect(9.0, 9.0, 21.0, 21.0);
                self.fill(&[(9.0, 9.0), (15.0, 9.0), (15.0, 15.0), (9.0, 15.0)]);
            }
            Subtract => {
                self.rect(3.0, 3.0, 15.0, 15.0);
                self.dashed_rect(9.0, 9.0, 21.0, 21.0);
            }
            Hollow => {
                self.rect(3.0, 3.0, 21.0, 21.0);
                self.rect(7.0, 7.0, 17.0, 17.0);
            }
            Material => {
                self.circle(12.0, 8.0, 5.0);
                self.line(&[(12.0, 13.0), (12.0, 21.0)]);
                self.dot(12.0, 8.0, 1.5);
            }
            Color => {
                self.closed(&[(5.0, 4.0), (19.0, 4.0), (16.0, 14.0), (8.0, 14.0)]);
                self.line(&[(9.0, 14.0), (12.0, 21.0), (15.0, 14.0)]);
                self.line(&[
                    (5.0, 4.0),
                    (8.0, 9.0),
                    (12.0, 4.0),
                    (16.0, 9.0),
                    (19.0, 4.0),
                ]);
            }
            Texture => {
                self.line(&[(21.0, 3.0), (11.0, 13.0)]);
                self.closed(&[
                    (11.0, 13.0),
                    (13.0, 15.0),
                    (8.0, 21.0),
                    (3.0, 21.0),
                    (5.0, 17.0),
                ]);
                self.line(&[(6.0, 19.0), (9.0, 16.0)]);
            }
            Eye => {
                self.ellipse(12.0, 12.0, 9.0, 5.0, 0.0, TAU);
                self.dot(12.0, 12.0, 2.5);
            }
            Shading => {
                self.circle(12.0, 12.0, 8.5);
                self.dot(9.0, 11.0, 1.0);
                self.dot(15.0, 11.0, 1.0);
                self.arc(12.0, 12.0, 4.5, 0.3, PI - 0.3);
                self.arc(12.0, 12.0, 8.5, -PI * 0.9, -PI * 0.1);
                self.line(&[(5.0, 7.0), (19.0, 7.0)]);
            }
            Shadows => {
                self.arc(12.0, 9.0, 6.0, PI * 0.8, PI * 2.2);
                self.line(&[(7.2, 12.5), (9.0, 17.0), (15.0, 17.0), (16.8, 12.5)]);
                self.line(&[(9.5, 20.0), (14.5, 20.0)]);
            }
            Colors => {
                self.closed(&[(4.0, 4.0), (20.0, 4.0), (17.0, 12.0), (7.0, 12.0)]);
                self.line(&[(9.0, 12.0), (9.0, 20.0)]);
                self.line(&[(15.0, 12.0), (15.0, 20.0)]);
                self.line(&[(4.0, 20.0), (20.0, 20.0)]);
            }
            Reflections => {
                self.arc(12.0, 16.0, 5.0, PI, TAU);
                self.line(&[(2.0, 16.0), (22.0, 16.0)]);
                self.line(&[(5.0, 20.0), (19.0, 20.0)]);
                for (x, y) in [(4.0, 9.0), (12.0, 5.0), (20.0, 9.0)] {
                    self.line(&[(x, y), (x + (12.0 - x) * 0.25, y + (16.0 - y) * 0.25)]);
                }
            }
            Smoothing => {
                self.closed(&[(12.0, 2.0), (20.0, 12.0), (12.0, 22.0), (4.0, 12.0)]);
                self.line(&[(4.0, 12.0), (20.0, 12.0)]);
                self.line(&[(12.0, 2.0), (12.0, 22.0)]);
            }
            Wireframe => {
                self.circle(12.0, 12.0, 9.0);
                self.ellipse(12.0, 12.0, 4.0, 9.0, 0.0, TAU);
                self.line(&[(3.0, 12.0), (21.0, 12.0)]);
                self.ellipse(12.0, 7.5, 7.5, 1.5, 0.0, PI);
                self.ellipse(12.0, 16.5, 7.5, 1.5, 0.0, PI);
            }
            Grid => {
                self.closed(&[(6.0, 4.0), (18.0, 4.0), (21.0, 20.0), (3.0, 20.0)]);
                self.line(&[(4.0, 10.0), (20.0, 10.0)]);
                self.line(&[(3.5, 15.0), (20.5, 15.0)]);
                self.line(&[(10.0, 4.0), (9.0, 20.0)]);
                self.line(&[(14.0, 4.0), (15.0, 20.0)]);
            }
            XRay => {
                self.line(&[(12.0, 3.0), (12.0, 21.0)]);
                self.line(&[(4.0, 7.5), (20.0, 16.5)]);
                self.line(&[(4.0, 16.5), (20.0, 7.5)]);
            }
            Help => {
                self.arc(12.0, 8.0, 5.0, PI, TAU + 0.6);
                self.line(&[(16.0, 11.0), (12.0, 14.0), (12.0, 16.0)]);
                self.dot(12.0, 20.0, 1.2);
            }
            Info => {
                self.circle(12.0, 12.0, 9.0);
                self.line(&[(12.0, 10.0), (12.0, 17.0)]);
                self.dot(12.0, 7.0, 1.1);
            }
            Guide => {
                self.closed(&[(2.0, 9.0), (12.0, 4.0), (22.0, 9.0), (12.0, 14.0)]);
                self.line(&[
                    (6.0, 11.0),
                    (6.0, 17.0),
                    (12.0, 20.0),
                    (18.0, 17.0),
                    (18.0, 11.0),
                ]);
                self.line(&[(22.0, 9.0), (22.0, 15.0)]);
            }
            Group => {
                self.dashed_rect(3.0, 3.0, 21.0, 21.0);
                self.rect(7.0, 7.0, 13.0, 13.0);
                self.fill(&[(11.0, 11.0), (17.0, 11.0), (17.0, 17.0), (11.0, 17.0)]);
            }
            Ungroup => {
                self.dashed_rect(3.0, 3.0, 21.0, 21.0);
                self.fill(&[(7.0, 7.0), (12.0, 7.0), (12.0, 12.0), (7.0, 12.0)]);
                self.rect(13.0, 13.0, 18.0, 18.0);
            }
            SelectAll => {
                self.circle(12.0, 12.0, 9.0);
                self.closed(&[(12.0, 7.0), (16.0, 15.0), (8.0, 15.0)]);
                self.line(&[(15.0, 7.0), (17.0, 9.0)]);
            }
            SelectNone => self.circle(12.0, 12.0, 9.0),
            Invert => {
                self.circle(12.0, 12.0, 9.0);
                self.fill(&[(12.0, 7.0), (16.0, 15.0), (8.0, 15.0)]);
            }
            SpecialSelect => {
                self.circle(12.0, 12.0, 9.0);
                self.closed(&[(12.0, 7.0), (16.0, 15.0), (8.0, 15.0)]);
            }
            NewScene => {
                self.line(&[(12.0, 3.0), (12.0, 13.0)]);
                self.arrow_head(12.0, 13.0, 0.0, 1.0);
                self.line(&[(4.0, 17.0), (12.0, 21.0), (20.0, 17.0)]);
                self.line(&[(4.0, 13.0), (8.0, 15.0)]);
                self.line(&[(20.0, 13.0), (16.0, 15.0)]);
            }
            Open => {
                self.line(&[
                    (4.0, 21.0),
                    (4.0, 3.0),
                    (14.0, 3.0),
                    (19.0, 8.0),
                    (19.0, 12.0),
                ]);
                self.line(&[(14.0, 3.0), (14.0, 8.0), (19.0, 8.0)]);
                self.closed(&[(8.0, 21.0), (10.0, 14.0), (22.0, 14.0), (20.0, 21.0)]);
            }
            Cart => {
                self.line(&[
                    (2.0, 4.0),
                    (5.0, 4.0),
                    (8.0, 16.0),
                    (19.0, 16.0),
                    (21.0, 8.0),
                    (6.0, 8.0),
                ]);
                self.circle(9.0, 19.5, 1.5);
                self.circle(18.0, 19.5, 1.5);
            }
            Printer => {
                self.rect(7.0, 3.0, 17.0, 8.0);
                self.rect(3.0, 8.0, 21.0, 17.0);
                self.rect(7.0, 13.0, 17.0, 21.0);
            }
            AboutModel => {
                self.rect(3.0, 6.0, 21.0, 18.0);
                for x in [8.0, 12.0, 16.0] {
                    self.dot(x, 14.0, 0.9);
                }
            }
            PrinterStatus => {
                self.line(&[(3.0, 6.0), (5.0, 8.0), (8.0, 4.0)]);
                self.line(&[(10.0, 6.0), (21.0, 6.0)]);
                self.line(&[(4.0, 12.0), (21.0, 12.0)]);
                self.line(&[(4.0, 18.0), (21.0, 18.0)]);
            }
            Feedback => {
                self.circle(9.0, 8.0, 4.0);
                self.line(&[(2.0, 21.0), (3.0, 16.0), (9.0, 13.0), (13.0, 14.0)]);
                self.rect(14.0, 12.0, 22.0, 18.0);
                self.line(&[(16.0, 18.0), (15.0, 21.0), (19.0, 18.0)]);
            }
            Assistant => {
                // A speech bubble with a spark in it.
                self.closed(&[
                    (3.0, 4.0),
                    (21.0, 4.0),
                    (21.0, 17.0),
                    (10.0, 17.0),
                    (5.0, 21.0),
                    (6.0, 17.0),
                    (3.0, 17.0),
                ]);
                self.closed(&[
                    (12.0, 6.5),
                    (13.2, 9.3),
                    (16.0, 10.5),
                    (13.2, 11.7),
                    (12.0, 14.5),
                    (10.8, 11.7),
                    (8.0, 10.5),
                    (10.8, 9.3),
                ]);
            }
            Settings => {
                self.circle(12.0, 12.0, 3.0);
                let pts: Vec<(f32, f32)> = (0..32)
                    .map(|i| {
                        let a = i as f32 * TAU / 32.0;
                        let r = if (i / 2) % 2 == 0 { 9.5 } else { 7.0 };
                        (12.0 + r * a.cos(), 12.0 + r * a.sin())
                    })
                    .collect();
                self.closed(&pts);
            }
            Smile => {
                self.circle(12.0, 12.0, 9.0);
                self.dot(9.0, 10.0, 1.0);
                self.dot(15.0, 10.0, 1.0);
                self.arc(12.0, 12.0, 5.0, 0.4, PI - 0.4);
            }
            Move => {
                self.rect(5.0, 12.0, 13.0, 20.0);
                self.line(&[(9.0, 12.0), (9.0, 4.0)]);
                self.arrow_head(9.0, 4.0, 0.0, -1.0);
                self.line(&[(13.0, 16.0), (21.0, 16.0)]);
                self.arrow_head(21.0, 16.0, 1.0, 0.0);
            }
            Rotate => {
                self.arc(12.0, 12.0, 8.0, -PI * 0.35, PI * 1.55);
                self.line(&[(15.0, 3.0), (15.5, 7.0), (19.5, 6.5)]);
            }
            Scale => {
                self.rect(6.0, 6.0, 18.0, 18.0);
                self.line(&[(3.0, 3.0), (7.0, 7.0)]);
                self.line(&[(21.0, 3.0), (17.0, 7.0)]);
                self.line(&[(3.0, 21.0), (7.0, 17.0)]);
                self.line(&[(21.0, 21.0), (17.0, 17.0)]);
                self.line(&[(3.0, 3.0), (6.0, 3.0)]);
                self.line(&[(3.0, 3.0), (3.0, 6.0)]);
                self.line(&[(21.0, 21.0), (18.0, 21.0)]);
                self.line(&[(21.0, 21.0), (21.0, 18.0)]);
            }
            Lock => {
                self.rect(5.0, 11.0, 19.0, 21.0);
                self.arc(12.0, 9.0, 4.5, PI, TAU);
                self.line(&[(7.5, 9.0), (7.5, 11.0)]);
                self.line(&[(16.5, 9.0), (16.5, 11.0)]);
            }
            Unlock => {
                // The shackle lifted and open on the right, with the keyhole dot.
                self.rect(5.0, 11.0, 19.0, 21.0);
                self.arc(12.0, 7.0, 4.5, PI, TAU);
                self.line(&[(7.5, 7.0), (7.5, 11.0)]);
                self.dot(12.0, 16.0, 1.2);
            }
            Axes => {
                // Up, right and towards the viewer, each with an arrowhead.
                self.line(&[(9.0, 15.0), (9.0, 3.0)]);
                self.line(&[(6.0, 6.0), (9.0, 3.0), (12.0, 6.0)]);
                self.line(&[(9.0, 15.0), (21.0, 15.0)]);
                self.line(&[(18.0, 12.0), (21.0, 15.0), (18.0, 18.0)]);
                self.line(&[(9.0, 15.0), (3.0, 21.0)]);
                self.fill(&[(3.0, 21.0), (3.0, 16.5), (7.5, 21.0)]);
            }
            FileUp => {
                // The page with its corner folded, open at the bottom right for the arrow.
                self.line(&[
                    (14.0, 21.0),
                    (4.0, 21.0),
                    (4.0, 3.0),
                    (13.0, 3.0),
                    (19.0, 9.0),
                    (19.0, 13.0),
                ]);
                self.line(&[(13.0, 3.0), (13.0, 9.0), (19.0, 9.0)]);
                self.line(&[(18.0, 23.0), (18.0, 15.0)]);
                self.line(&[(15.0, 18.0), (18.0, 15.0), (21.0, 18.0)]);
            }
            Wrench => {
                self.arc(16.0, 8.0, 5.0, PI * 0.75, PI * 2.25);
                self.line(&[(12.5, 11.5), (4.0, 20.0)]);
                self.line(&[(14.0, 13.0), (5.5, 21.5)]);
            }
            Warning => {
                self.closed(&[(12.0, 3.0), (22.0, 20.0), (2.0, 20.0)]);
                self.line(&[(12.0, 9.0), (12.0, 14.5)]);
                self.dot(12.0, 17.5, 1.1);
            }
            ZoomIn | ZoomOut => {
                self.circle(10.0, 10.0, 7.0);
                self.line(&[(15.0, 15.0), (21.0, 21.0)]);
                self.line(&[(7.0, 10.0), (13.0, 10.0)]);
                if icon == ZoomIn {
                    self.line(&[(10.0, 7.0), (10.0, 13.0)]);
                }
            }
            Pencil => {
                self.closed(&[
                    (4.0, 20.0),
                    (5.0, 15.0),
                    (16.0, 4.0),
                    (20.0, 8.0),
                    (9.0, 19.0),
                ]);
                self.line(&[(14.0, 6.0), (18.0, 10.0)]);
            }
            More => {
                for x in [5.0, 12.0, 19.0] {
                    self.dot(x, 12.0, 1.4);
                }
            }
            KeepTop => {
                self.fill(&[(12.0, 3.0), (15.0, 12.0), (9.0, 12.0)]);
                self.line(&[(7.0, 15.0), (17.0, 15.0)]);
                self.line(&[(5.0, 19.0), (19.0, 19.0)]);
            }
        }
    }
}
