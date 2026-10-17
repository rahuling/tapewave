//! All application colours are semantic tokens, persisted independently of layout.
use eframe::egui::{self, Color32, Stroke};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Palette {
    pub background: [u8; 3],
    pub panel: [u8; 3],
    pub text: [u8; 3],
    pub secondary: [u8; 3],
    pub accent: [u8; 3],
    pub border: [u8; 3],
    pub selection: [u8; 3],
    pub warning: [u8; 3],
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            background: [29, 31, 33],
            panel: [39, 42, 45],
            text: [241, 240, 235],
            secondary: [173, 178, 176],
            accent: [168, 201, 138],
            border: [65, 70, 74],
            selection: [48, 55, 53],
            warning: [230, 180, 92],
        }
    }
}

pub fn color(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

pub fn contrast(a: [u8; 3], b: [u8; 3]) -> f32 {
    fn luminance(rgb: [u8; 3]) -> f32 {
        let c = rgb.map(|v| {
            let s = v as f32 / 255.0;
            if s <= 0.04045 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        });
        0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
    }
    let a = luminance(a);
    let b = luminance(b);
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

impl Palette {
    pub fn light() -> Self {
        Self {
            background: [242, 241, 235],
            panel: [232, 232, 224],
            text: [32, 37, 30],
            secondary: [85, 93, 81],
            accent: [71, 99, 45],
            border: [160, 166, 155],
            selection: [210, 220, 201],
            warning: [134, 79, 16],
        }
    }
    pub fn readable(&self) -> bool {
        [self.background, self.panel, self.selection]
            .into_iter()
            .all(|surface| {
                contrast(self.text, surface) >= 4.5 && contrast(self.secondary, surface) >= 4.5
            })
            && contrast(self.accent, self.background) >= 3.0
            && contrast(self.warning, self.background) >= 3.0
    }
    pub fn apply(&self, ctx: &egui::Context) {
        let mut visuals =
            if contrast([0, 0, 0], self.background) > contrast([255, 255, 255], self.background) {
                egui::Visuals::light()
            } else {
                egui::Visuals::dark()
            };
        visuals.override_text_color = Some(color(self.text));
        visuals.panel_fill = color(self.background);
        visuals.window_fill = color(self.panel);
        visuals.extreme_bg_color = color(self.background);
        visuals.faint_bg_color = color(self.panel);
        visuals.selection.bg_fill = color(self.selection);
        visuals.selection.stroke = Stroke::new(1.0, color(self.accent));
        visuals.hyperlink_color = color(self.accent);
        visuals.warn_fg_color = color(self.warning);
        for widget in [
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.noninteractive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
            &mut visuals.widgets.open,
        ] {
            widget.bg_fill = color(self.panel);
            widget.weak_bg_fill = color(self.panel);
            widget.bg_stroke = Stroke::new(1.0, color(self.border));
            widget.fg_stroke = Stroke::new(1.0, color(self.text));
            widget.corner_radius = 4.into();
        }
        visuals.widgets.hovered.bg_fill = color(self.selection);
        visuals.widgets.active.bg_stroke = Stroke::new(1.0, color(self.accent));
        ctx.set_visuals(visuals);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_palettes_have_readable_text() {
        assert!(Palette::default().readable());
        assert!(Palette::light().readable());
    }
    #[test]
    fn rejects_an_invisible_palette() {
        let mut p = Palette::default();
        p.text = p.background;
        assert!(!p.readable());
        assert!((contrast([0, 0, 0], [255, 255, 255]) - 21.0).abs() < 0.01);
    }
}
