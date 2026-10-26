use eframe::egui::{self, Color32, CornerRadius, Frame, Margin, Shadow, Stroke};

pub mod token {
    pub const SPACE_6: f32 = 32.0;
    pub const RADIUS_MD: u8 = 8;
    pub const RADIUS_SM: u8 = 4;
    pub const HIT: f32 = 32.0;
}

#[derive(Clone, Debug)]
pub struct Palette {
    pub background: Color32,
    pub panel: Color32,
    pub foreground: Color32,
    pub dim: Color32,
    pub comment: Color32,
    pub accent: Color32,
    pub number: Color32,
    pub keyword: Color32,
    pub variable: Color32,
    pub defined: Color32,
    pub selection: Color32,
    pub hover: Color32,
}

impl Default for Palette {
    fn default() -> Self {
        Self::new()
    }
}

impl Palette {
    pub fn new() -> Self {
        Self {
            background: hex("#191033"),
            panel: hex("#211641"),
            foreground: hex("#f8f8f8"),
            dim: hex("#6e6194"),
            comment: hex("#4c3e76"),
            accent: hex("#fad000"),
            number: hex("#ffb454"),
            keyword: hex("#80fcff"),
            variable: hex("#fb94ff"),
            defined: hex("#a5ff90"),
            selection: hex("#32245d"),
            hover: hex("#28194a"),
        }
    }

    pub fn text(&self) -> Color32 {
        self.foreground
    }

    pub fn text_secondary(&self) -> Color32 {
        self.dim
    }

    pub fn surface(&self) -> Color32 {
        self.panel
    }

    pub fn border(&self) -> Color32 {
        hex("#4c3e76")
    }

    pub fn border_hover(&self) -> Color32 {
        mix(self.border(), self.variable, 0.35)
    }

    pub fn danger(&self) -> Color32 {
        hex("#ff628c")
    }

    pub fn warning(&self) -> Color32 {
        hex("#fad000")
    }
}

fn hex(s: &str) -> Color32 {
    let h = s.strip_prefix('#').unwrap_or(s);
    let n = u32::from_str_radix(h, 16).unwrap_or(0);
    Color32::from_rgb(
        ((n >> 16) & 0xff) as u8,
        ((n >> 8) & 0xff) as u8,
        (n & 0xff) as u8,
    )
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let at = 1.0 - t;
    Color32::from_rgb(
        (a.r() as f32 * at + b.r() as f32 * t) as u8,
        (a.g() as f32 * at + b.g() as f32 * t) as u8,
        (a.b() as f32 * at + b.b() as f32 * t) as u8,
    )
}

pub fn shadow() -> Shadow {
    Shadow {
        offset: [0, 4],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(80),
    }
}

pub fn popup(p: &Palette) -> Frame {
    Frame::NONE
        .fill(p.surface())
        .stroke(Stroke::new(1.0, p.border()))
        .corner_radius(CornerRadius::same(token::RADIUS_MD))
        .inner_margin(Margin::same(8))
        .shadow(shadow())
}

pub fn notice(p: &Palette) -> Frame {
    Frame::NONE
        .fill(p.hover)
        .corner_radius(CornerRadius::same(token::RADIUS_SM))
        .inner_margin(Margin::same(6))
}

pub fn field(p: &Palette) -> Frame {
    Frame::NONE
        .fill(p.surface())
        .stroke(Stroke::new(1.5, p.accent))
        .corner_radius(CornerRadius::same(token::RADIUS_MD))
        .inner_margin(Margin::same(8))
        .shadow(shadow())
}

pub fn radius_row() -> CornerRadius {
    CornerRadius::same(token::RADIUS_MD)
}

pub fn apply(ctx: &egui::Context, p: &Palette) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = p.background;
    visuals.window_fill = p.surface();
    visuals.extreme_bg_color = p.background;
    visuals.faint_bg_color = p.hover;
    visuals.code_bg_color = p.surface();
    visuals.override_text_color = Some(p.text());
    visuals.selection.bg_fill = p.selection;
    visuals.selection.stroke = Stroke::new(1.0, p.accent);
    visuals.hyperlink_color = p.accent;
    visuals.error_fg_color = p.danger();
    visuals.warn_fg_color = p.warning();
    visuals.popup_shadow = shadow();

    visuals.window_stroke = Stroke::new(1.0, p.border());
    visuals.text_cursor.stroke.color = p.accent;
    visuals.text_cursor.stroke.width = 2.0;

    let radius = radius_row();
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = radius;
        widget.fg_stroke.color = p.text();
        widget.bg_stroke = Stroke::new(1.0, p.border());
    }
    visuals.widgets.noninteractive.fg_stroke.color = p.text_secondary();
    visuals.widgets.inactive.weak_bg_fill = p.background;
    visuals.widgets.inactive.bg_fill = p.background;
    visuals.widgets.hovered.weak_bg_fill = p.hover;
    visuals.widgets.hovered.bg_fill = p.hover;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, p.border_hover());
    visuals.widgets.active.weak_bg_fill = p.hover;
    visuals.widgets.active.bg_fill = p.selection;
    visuals.widgets.active.bg_stroke = Stroke::new(1.5, p.accent);
    ctx.set_visuals(visuals);
}

pub const GAP: f32 = token::SPACE_6;

pub fn swatch_room(font: &egui::FontId) -> f32 {
    font.size * 0.8 + 8.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spread(c: Color32) -> i32 {
        let channels = [c.r() as i32, c.g() as i32, c.b() as i32];
        channels.iter().max().unwrap() - channels.iter().min().unwrap()
    }

    #[test]
    fn the_edge_is_the_violet_the_palette_asks_for() {
        let p = Palette::new();
        assert_eq!(p.border(), hex("#4c3e76"));
        for edge in [p.border(), p.border_hover()] {
            assert!(
                spread(edge) >= 0x30,
                "{edge:?} is too close to grey to read as an edge"
            );
            assert!(
                edge.b() > p.surface().b() && edge.r() > p.surface().r(),
                "{edge:?} should sit above the panel it edges"
            );
        }
        assert!(p.border_hover().b() > p.border().b());
    }
}
