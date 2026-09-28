use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, Rect};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, measure_text_with_fallback, paint_from_hex,
    typeface_with_fallback,
};
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

const LIMB_VISIBILITY_EPSILON: f64 = 0.0;
const ARC_STEPS: usize = 96;
const ARC_STROKE_WIDTH: f32 = 2.5;
const ARC_DEFAULT_ALTITUDE: f32 = 0.12;
const ARC_DEFAULT_COLOR: &str = "#F97316";
const ORTHOGRAPHIC_DISC_MARGIN: f32 = 0.94;

fn default_arc_draw_in() -> f32 {
    1.0
}

/// `dot_map.projection`. `Equirectangular` is the historical flat mapping
/// (`lng`/`lat` scaled linearly onto the box); `Orthographic` renders the map
/// as a globe seen from outside, with the far hemisphere culled and dots
/// converging towards the limb — a `clip-path: circle` on the flat map only
/// cuts a disc out of it, it never curves.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DotMapProjection {
    #[default]
    Equirectangular,
    Orthographic,
}

/// A `rotate.lng` value: either a fixed longitude, or a linear sweep from
/// `from` to `to` starting at the component's own `start_at`, over
/// `duration` seconds (defaults to `animation_duration`).
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum RotationAngle {
    Fixed(f32),
    Animated {
        from: f32,
        to: f32,
        #[serde(default)]
        duration: Option<f64>,
    },
}

impl RotationAngle {
    fn value_at(&self, time: f64, start_at: f64, fallback_duration: f64) -> f32 {
        match self {
            RotationAngle::Fixed(v) => *v,
            RotationAngle::Animated { from, to, duration } => {
                let dur = duration.unwrap_or(fallback_duration).max(0.0001);
                let elapsed = (time - start_at).max(0.0);
                let p = (elapsed / dur).clamp(0.0, 1.0) as f32;
                from + (to - from) * p
            }
        }
    }
}

/// `dot_map.rotate` in `orthographic` mode: `lng` is rotation about the
/// pole (the sub-observer meridian), `lat` tilts which parallel faces the
/// camera. Both default to 0 (looking at `(0, 0)`).
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GlobeRotation {
    #[serde(default)]
    pub lng: Option<RotationAngle>,
    #[serde(default)]
    pub lat: Option<f32>,
}

/// A great-circle arc lifted off the globe's surface, drawn between two
/// `[lat, lng]` endpoints. `draw_in` reveals the arc from `from` towards
/// `to` (1.0 = fully drawn), the counterpart of `draw_progress` on `line`/
/// `arrow` — a plain static fraction here, since an arc's shape (which
/// hemisphere it crosses) already changes as the globe itself rotates.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GreatCircleArc {
    pub from: [f64; 2],
    pub to: [f64; 2],
    #[serde(default = "default_arc_draw_in")]
    pub draw_in: f32,
    #[serde(default)]
    pub color: Option<String>,
    /// How far the arc lifts off the surface at its midpoint, as a fraction
    /// of the globe's radius. Default 0.12.
    #[serde(default)]
    pub altitude: Option<f32>,
}

fn latlng_to_unit(lat_deg: f64, lng_deg: f64) -> (f64, f64, f64) {
    let lat = lat_deg.to_radians();
    let lng = lng_deg.to_radians();
    (lat.cos() * lng.cos(), lat.cos() * lng.sin(), lat.sin())
}

fn unit_to_latlng(v: (f64, f64, f64)) -> (f64, f64) {
    let lat = v.2.clamp(-1.0, 1.0).asin().to_degrees();
    let lng = v.1.atan2(v.0).to_degrees();
    (lat, lng)
}

fn slerp_unit(a: (f64, f64, f64), b: (f64, f64, f64), t: f64) -> (f64, f64, f64) {
    let dot = (a.0 * b.0 + a.1 * b.1 + a.2 * b.2).clamp(-1.0, 1.0);
    let theta = dot.acos();
    if theta.abs() < 1e-9 {
        return a;
    }
    let sin_theta = theta.sin();
    let wa = ((1.0 - t) * theta).sin() / sin_theta;
    let wb = (t * theta).sin() / sin_theta;
    (
        a.0 * wa + b.0 * wb,
        a.1 * wa + b.1 * wb,
        a.2 * wa + b.2 * wb,
    )
}

/// The closed-form orthographic projection: `(lat, lng)` centred on
/// `(center_lat, center_lng)`, returned as normalized `(x, y)` on a unit
/// sphere (multiply by the disc's pixel radius) plus `cos_c`, the cosine of
/// the angular distance from the sub-observer point — `1.0` at the centre
/// of the disc, `0.0` at the limb, negative on the far hemisphere. `None`
/// means the point is on the far hemisphere and must not be drawn.
fn orthographic_project(
    lat_deg: f64,
    lng_deg: f64,
    center_lat_deg: f64,
    center_lng_deg: f64,
) -> Option<(f64, f64, f64)> {
    let phi = lat_deg.to_radians();
    let lambda = lng_deg.to_radians();
    let phi1 = center_lat_deg.to_radians();
    let lambda0 = center_lng_deg.to_radians();
    let dlambda = lambda - lambda0;
    let cos_c = phi1.sin() * phi.sin() + phi1.cos() * phi.cos() * dlambda.cos();
    if cos_c < LIMB_VISIBILITY_EPSILON {
        return None;
    }
    let x = phi.cos() * dlambda.sin();
    let y = phi1.cos() * phi.sin() - phi1.sin() * phi.cos() * dlambda.cos();
    Some((x, y, cos_c))
}

fn limb_shade(limb_shading: Option<f32>, cos_c: f64) -> f32 {
    let strength = limb_shading.unwrap_or(0.0).clamp(0.0, 1.0);
    (1.0 - strength * (1.0 - cos_c as f32)).clamp(0.0, 1.0)
}

fn default_background_color() -> String {
    "#0F172A".to_string()
}

fn default_dot_color() -> String {
    "#334155".to_string()
}

fn default_dot_spacing() -> f32 {
    8.0
}

fn default_dot_radius() -> f32 {
    1.5
}

fn default_animated() -> bool {
    true
}

fn default_animation_duration() -> f64 {
    1.5
}

fn default_show_world() -> bool {
    true
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct MapPoint {
    /// Latitude (-90 to 90, positive = north)
    pub lat: f64,
    /// Longitude (-180 to 180, positive = east)
    pub lng: f64,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub size: Option<f32>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub pulse: Option<bool>,
}

const LAT_MAX: f64 = 85.0;
const LAT_MIN: f64 = -85.0;

fn geo_to_screen(lat: f64, lng: f64) -> (f32, f32) {
    let x = ((lng + 180.0) / 360.0) as f32;
    let y = ((LAT_MAX - lat) / (LAT_MAX - LAT_MIN)) as f32;
    (x, y)
}

fn screen_to_geo(nx: f32, ny: f32) -> (f64, f64) {
    let lng = nx as f64 * 360.0 - 180.0;
    let lat = LAT_MAX - ny as f64 * (LAT_MAX - LAT_MIN);
    (lat, lng)
}

fn geo_is_land(lat: f64, lng: f64) -> bool {
    let bitmap = super::world_bitmap::land_bitmap();
    let col = ((lng + 180.0) / 360.0 * 180.0) as usize;
    let row = ((90.0 - lat) / 180.0 * 90.0) as usize;
    if col >= 180 || row >= 90 {
        return false;
    }
    bitmap[row * 180 + col] == 1
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct DotMap {
    pub points: Vec<MapPoint>,
    #[serde(default = "default_background_color")]
    pub background_color: String,
    /// Color of the world map dots
    #[serde(default = "default_dot_color")]
    pub world_dot_color: String,
    /// Spacing between world map dots in pixels
    #[serde(default = "default_dot_spacing")]
    pub dot_spacing: f32,
    /// Radius of each world map dot
    #[serde(default = "default_dot_radius")]
    pub dot_radius: f32,
    /// Show world map dot pattern
    #[serde(default = "default_show_world")]
    pub show_world: bool,
    #[serde(default = "default_animated")]
    pub animated: bool,
    #[serde(default = "default_animation_duration")]
    pub animation_duration: f64,
    /// `equirectangular` (default, flat — unchanged) or `orthographic` (a
    /// globe seen from outside: closed-form lat/lng-to-screen projection,
    /// far hemisphere culled).
    #[serde(default)]
    pub projection: DotMapProjection,
    /// Orthographic-only: which meridian/parallel faces the camera. Inert
    /// under `equirectangular`.
    #[serde(default)]
    pub rotate: Option<GlobeRotation>,
    /// Orthographic-only: darkens dots towards the limb, 0 (none, default)
    /// to 1 (full black at the very edge). Inert under `equirectangular`.
    #[serde(default)]
    pub limb_shading: Option<f32>,
    /// Orthographic-only: great-circle arcs lifted off the surface. Inert
    /// under `equirectangular`.
    #[serde(default)]
    pub arcs: Vec<GreatCircleArc>,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

rustmotion_core::impl_traits!(DotMap {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

fn is_land_screen(nx: f32, ny: f32) -> bool {
    let (lat, lng) = screen_to_geo(nx, ny);
    geo_is_land(lat, lng)
}

impl DotMap {
    fn progress_at(&self, time: f64) -> f32 {
        if !self.animated {
            return 1.0;
        }
        let start = self.timing.start_at.unwrap_or(0.0);
        let elapsed = (time - start).max(0.0);
        let p = (elapsed / self.animation_duration).clamp(0.0, 1.0) as f32;
        1.0 - (1.0 - p).powi(3)
    }

    fn paint(&self, canvas: &Canvas, layout_w: f32, layout_h: f32, ctx: &PaintCtx) {
        match self.projection {
            DotMapProjection::Equirectangular => {
                self.paint_equirectangular(canvas, layout_w, layout_h, ctx.time)
            }
            DotMapProjection::Orthographic => {
                self.paint_orthographic(canvas, layout_w, layout_h, ctx.time)
            }
        }
    }

    fn globe_center(&self, time: f64) -> (f64, f64) {
        let start_at = self.timing.start_at.unwrap_or(0.0);
        let center_lng = self
            .rotate
            .as_ref()
            .and_then(|r| r.lng.as_ref())
            .map(|a| a.value_at(time, start_at, self.animation_duration) as f64)
            .unwrap_or(0.0);
        let center_lat = self
            .rotate
            .as_ref()
            .and_then(|r| r.lat)
            .map(|v| v as f64)
            .unwrap_or(0.0);
        (center_lat, center_lng)
    }

    fn paint_orthographic(&self, canvas: &Canvas, layout_w: f32, layout_h: f32, time: f64) {
        let w = layout_w;
        let h = layout_h;
        let progress = self.progress_at(time);

        let mut bg_paint = paint_from_hex(&self.background_color);
        bg_paint.set_style(PaintStyle::Fill);
        bg_paint.set_anti_alias(true);
        canvas.draw_rect(Rect::from_xywh(0.0, 0.0, w, h), &bg_paint);

        let radius = (w.min(h) * 0.5 * ORTHOGRAPHIC_DISC_MARGIN).max(1.0);
        let center = (w / 2.0, h / 2.0);
        let (center_lat, center_lng) = self.globe_center(time);

        if self.show_world {
            self.paint_world_dots_orthographic(canvas, center, radius, center_lat, center_lng);
        }

        self.paint_points_orthographic(
            canvas, center, radius, center_lat, center_lng, progress, time,
        );
        self.paint_arcs_orthographic(canvas, center, radius, center_lat, center_lng);
    }

    fn paint_world_dots_orthographic(
        &self,
        canvas: &Canvas,
        center: (f32, f32),
        radius: f32,
        center_lat: f64,
        center_lng: f64,
    ) {
        let mut world_paint = paint_from_hex(&self.world_dot_color);
        world_paint.set_style(PaintStyle::Fill);
        world_paint.set_anti_alias(true);

        let step_rad = (self.dot_spacing.max(1.0) as f64 / radius as f64).max(0.002);
        let step_deg = step_rad.to_degrees().max(0.5);

        const MAX_STEPS_PER_AXIS: u32 = 2048;
        let lat_steps = ((170.0 / step_deg) as u32 + 1).min(MAX_STEPS_PER_AXIS);
        let lng_steps = ((360.0 / step_deg) as u32 + 1).min(MAX_STEPS_PER_AXIS);

        for lat_i in 0..=lat_steps {
            let lat = LAT_MIN + lat_i as f64 * step_deg;
            if lat > LAT_MAX {
                continue;
            }
            for lng_i in 0..=lng_steps {
                let lng = -180.0 + lng_i as f64 * step_deg;
                if lng > 180.0 {
                    continue;
                }
                if !geo_is_land(lat, lng) {
                    continue;
                }
                let Some((x, y, cos_c)) = orthographic_project(lat, lng, center_lat, center_lng)
                else {
                    continue;
                };
                let shade = limb_shade(self.limb_shading, cos_c);
                let px = center.0 + x as f32 * radius;
                let py = center.1 - y as f32 * radius;
                world_paint.set_alpha_f(shade);
                canvas.draw_circle((px, py), self.dot_radius, &world_paint);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_points_orthographic(
        &self,
        canvas: &Canvas,
        center: (f32, f32),
        radius: f32,
        center_lat: f64,
        center_lng: f64,
        progress: f32,
        time: f64,
    ) {
        let default_color = "#3B82F6";
        let default_dot_size = 10.0_f32;
        let label_font_size = 12.0_f32;
        let point_count = self.points.len();

        let font_style = skia_safe::FontStyle::normal();
        let Ok(typeface) = typeface_with_fallback("Inter", font_style) else {
            return;
        };
        let label_font = skia_safe::Font::from_typeface(typeface, label_font_size);
        let emoji_font =
            emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, label_font_size));

        for (i, point) in self.points.iter().enumerate() {
            let dot_alpha = if self.animated {
                let stagger_delay = if point_count > 1 {
                    (i as f64 / point_count as f64) * 0.6
                } else {
                    0.0
                };
                let dot_progress = ((time - stagger_delay) / (self.animation_duration * 0.4))
                    .clamp(0.0, 1.0) as f32;
                dot_progress * progress
            } else {
                1.0
            };

            if dot_alpha <= 0.0 {
                continue;
            }

            let Some((x, y, cos_c)) =
                orthographic_project(point.lat, point.lng, center_lat, center_lng)
            else {
                continue;
            };
            let dot_alpha = dot_alpha * limb_shade(self.limb_shading, cos_c);
            let px = center.0 + x as f32 * radius;
            let py = center.1 - y as f32 * radius;
            let dot_size = point.size.unwrap_or(default_dot_size);
            let color_str = point.color.as_deref().unwrap_or(default_color);

            if point.pulse.unwrap_or(false) {
                let num_rings = 2;
                for ring in 0..num_rings {
                    let phase = ((time * 1.5 + ring as f64 * 0.5).fract()) as f32;
                    let ring_radius = dot_size * (1.0 + phase * 2.5);
                    let ring_alpha = (1.0 - phase).max(0.0) * 0.4 * dot_alpha;

                    let mut pulse_paint = paint_from_hex(color_str);
                    pulse_paint.set_style(PaintStyle::Stroke);
                    pulse_paint.set_stroke_width(2.0);
                    pulse_paint.set_anti_alias(true);
                    pulse_paint.set_alpha_f(ring_alpha);
                    canvas.draw_circle((px, py), ring_radius, &pulse_paint);
                }
            }

            let mut dot_paint = paint_from_hex(color_str);
            dot_paint.set_style(PaintStyle::Fill);
            dot_paint.set_anti_alias(true);
            dot_paint.set_alpha_f(dot_alpha);
            canvas.draw_circle((px, py), dot_size / 2.0, &dot_paint);

            let mut border_paint = paint_from_hex("#FFFFFF");
            border_paint.set_style(PaintStyle::Stroke);
            border_paint.set_stroke_width(1.5);
            border_paint.set_anti_alias(true);
            border_paint.set_alpha_f(dot_alpha * 0.6);
            canvas.draw_circle((px, py), dot_size / 2.0, &border_paint);

            if let Some(label) = &point.label {
                let mut label_paint = paint_from_hex("#FFFFFF");
                label_paint.set_anti_alias(true);
                label_paint.set_alpha_f(dot_alpha * 0.9);

                let text_w = measure_text_with_fallback(label, &label_font, &emoji_font, 0.0);
                let label_x = px - text_w / 2.0;
                let label_y = py + dot_size / 2.0 + label_font_size + 4.0;

                draw_text_with_fallback(
                    canvas,
                    label,
                    &label_font,
                    &emoji_font,
                    0.0,
                    label_x,
                    label_y,
                    &label_paint,
                );
            }
        }
    }

    fn paint_arcs_orthographic(
        &self,
        canvas: &Canvas,
        center: (f32, f32),
        radius: f32,
        center_lat: f64,
        center_lng: f64,
    ) {
        for arc in &self.arcs {
            let max_t = arc.draw_in.clamp(0.0, 1.0) as f64;
            if max_t <= 0.0 {
                continue;
            }
            let a = latlng_to_unit(arc.from[0], arc.from[1]);
            let b = latlng_to_unit(arc.to[0], arc.to[1]);
            let altitude = arc.altitude.unwrap_or(ARC_DEFAULT_ALTITUDE);
            let color = arc.color.as_deref().unwrap_or(ARC_DEFAULT_COLOR);

            let mut paint = paint_from_hex(color);
            paint.set_style(PaintStyle::Stroke);
            paint.set_stroke_width(ARC_STROKE_WIDTH);
            paint.set_anti_alias(true);
            paint.set_stroke_cap(skia_safe::PaintCap::Round);

            let mut prev: Option<(f32, f32)> = None;
            for i in 0..=ARC_STEPS {
                let s = (i as f64 / ARC_STEPS as f64) * max_t;
                let v = slerp_unit(a, b, s);
                let lift = altitude as f64 * (std::f64::consts::PI * s).sin();
                let (lat, lng) = unit_to_latlng(v);

                match orthographic_project(lat, lng, center_lat, center_lng) {
                    Some((x, y, cos_c)) => {
                        let r = radius * (1.0 + lift as f32);
                        let px = center.0 + x as f32 * r;
                        let py = center.1 - y as f32 * r;
                        if let Some(prev_pt) = prev {
                            paint.set_alpha_f(limb_shade(self.limb_shading, cos_c));
                            canvas.draw_line(prev_pt, (px, py), &paint);
                        }
                        prev = Some((px, py));
                    }
                    None => prev = None,
                }
            }
        }
    }

    fn paint_equirectangular(&self, canvas: &Canvas, layout_w: f32, layout_h: f32, time: f64) {
        let w = layout_w;
        let h = layout_h;
        let progress = self.progress_at(time);

        let mut bg_paint = paint_from_hex(&self.background_color);
        bg_paint.set_style(PaintStyle::Fill);
        bg_paint.set_anti_alias(true);
        canvas.draw_rect(Rect::from_xywh(0.0, 0.0, w, h), &bg_paint);

        if self.show_world {
            let mut world_paint = paint_from_hex(&self.world_dot_color);
            world_paint.set_style(PaintStyle::Fill);
            world_paint.set_anti_alias(true);

            let spacing = if self.dot_spacing.is_finite() && self.dot_spacing >= 1.0 {
                self.dot_spacing
            } else {
                1.0
            };
            let radius = self.dot_radius;
            let margin = spacing;

            const MAX_DOTS_PER_AXIS: u32 = 4096;
            let cols = (((w - margin * 2.0) / spacing) as u32).min(MAX_DOTS_PER_AXIS);
            let rows = (((h - margin * 2.0) / spacing) as u32).min(MAX_DOTS_PER_AXIS);

            for row in 0..rows {
                for col in 0..cols {
                    let px = margin + col as f32 * spacing;
                    let py = margin + row as f32 * spacing;

                    let nx = px / w;
                    let ny = py / h;

                    if is_land_screen(nx, ny) {
                        canvas.draw_circle((px, py), radius, &world_paint);
                    }
                }
            }
        }

        let default_color = "#3B82F6";
        let default_dot_size = 10.0_f32;
        let label_font_size = 12.0_f32;
        let point_count = self.points.len();

        let font_style = skia_safe::FontStyle::normal();
        let Ok(typeface) = typeface_with_fallback("Inter", font_style) else {
            return;
        };
        let label_font = skia_safe::Font::from_typeface(typeface, label_font_size);
        let emoji_font =
            emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, label_font_size));

        for (i, point) in self.points.iter().enumerate() {
            let dot_alpha = if self.animated {
                let stagger_delay = if point_count > 1 {
                    (i as f64 / point_count as f64) * 0.6
                } else {
                    0.0
                };
                let dot_progress = ((time - stagger_delay) / (self.animation_duration * 0.4))
                    .clamp(0.0, 1.0) as f32;
                dot_progress * progress
            } else {
                1.0
            };

            if dot_alpha <= 0.0 {
                continue;
            }

            let (nx, ny) = geo_to_screen(point.lat, point.lng);
            let px = nx * w;
            let py = ny * h;
            let dot_size = point.size.unwrap_or(default_dot_size);
            let color_str = point.color.as_deref().unwrap_or(default_color);

            if point.pulse.unwrap_or(false) {
                let num_rings = 2;
                for ring in 0..num_rings {
                    let phase = ((time * 1.5 + ring as f64 * 0.5).fract()) as f32;
                    let ring_radius = dot_size * (1.0 + phase * 2.5);
                    let ring_alpha = (1.0 - phase).max(0.0) * 0.4 * dot_alpha;

                    let mut pulse_paint = paint_from_hex(color_str);
                    pulse_paint.set_style(PaintStyle::Stroke);
                    pulse_paint.set_stroke_width(2.0);
                    pulse_paint.set_anti_alias(true);
                    pulse_paint.set_alpha_f(ring_alpha);
                    canvas.draw_circle((px, py), ring_radius, &pulse_paint);
                }
            }

            let mut dot_paint = paint_from_hex(color_str);
            dot_paint.set_style(PaintStyle::Fill);
            dot_paint.set_anti_alias(true);
            dot_paint.set_alpha_f(dot_alpha);
            canvas.draw_circle((px, py), dot_size / 2.0, &dot_paint);

            let mut border_paint = paint_from_hex("#FFFFFF");
            border_paint.set_style(PaintStyle::Stroke);
            border_paint.set_stroke_width(1.5);
            border_paint.set_anti_alias(true);
            border_paint.set_alpha_f(dot_alpha * 0.6);
            canvas.draw_circle((px, py), dot_size / 2.0, &border_paint);

            if let Some(label) = &point.label {
                let mut label_paint = paint_from_hex("#FFFFFF");
                label_paint.set_anti_alias(true);
                label_paint.set_alpha_f(dot_alpha * 0.9);

                let text_w = measure_text_with_fallback(label, &label_font, &emoji_font, 0.0);
                let label_x = px - text_w / 2.0;
                let label_y = py + dot_size / 2.0 + label_font_size + 4.0;

                draw_text_with_fallback(
                    canvas,
                    label,
                    &label_font,
                    &emoji_font,
                    0.0,
                    label_x,
                    label_y,
                    &label_paint,
                );
            }
        }
    }
}

impl Painter for DotMap {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        _props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        self.paint(canvas, layout.width, layout.height, ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ctx(time: f64) -> PaintCtx {
        PaintCtx {
            time,
            scenario_time: time,
            scene_duration: 2.0,
            frame_index: 0,
            fps: 30,
            video_width: 400,
            video_height: 400,
            stagger_offset: 0.0,
        }
    }

    fn render(map: &DotMap, w: u32, h: u32, time: f64) -> Vec<u8> {
        let mut surface = skia_safe::surfaces::raster_n32_premul((w as i32, h as i32)).unwrap();
        map.paint(surface.canvas(), w as f32, h as f32, &test_ctx(time));
        let info = skia_safe::ImageInfo::new(
            (w as i32, h as i32),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Unpremul,
            None,
        );
        let mut buf = vec![0u8; (w * h * 4) as usize];
        surface.read_pixels(&info, &mut buf, (w * 4) as usize, (0, 0));
        buf
    }

    fn dot_map_from(extra: serde_json::Value) -> DotMap {
        let mut base = serde_json::json!({
            "points": [],
            "animated": false,
        });
        base.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        serde_json::from_value(base).expect("dot_map json must deserialize")
    }

    #[test]
    fn projection_defaults_to_equirectangular() {
        assert_eq!(
            DotMapProjection::default(),
            DotMapProjection::Equirectangular
        );
        let map = dot_map_from(serde_json::json!({}));
        assert_eq!(map.projection, DotMapProjection::Equirectangular);
        assert!(map.rotate.is_none());
        assert!(map.limb_shading.is_none());
        assert!(map.arcs.is_empty());
    }

    #[test]
    fn omitting_the_new_fields_renders_byte_identically_to_declaring_them_at_default() {
        let implicit = dot_map_from(serde_json::json!({}));
        let explicit = dot_map_from(serde_json::json!({
            "projection": "equirectangular",
        }));
        let out_implicit = render(&implicit, 300, 300, 0.4);
        let out_explicit = render(&explicit, 300, 300, 0.4);
        assert_eq!(
            out_implicit, out_explicit,
            "a dot_map with no projection field must render exactly as one that spells out \
             the default explicitly — the new fields must not perturb the historical path"
        );
    }

    #[test]
    fn orthographic_hides_the_far_hemisphere() {
        assert!(orthographic_project(0.0, 0.0, 0.0, 0.0).is_some());
        assert!(
            orthographic_project(0.0, 170.0, 0.0, 0.0).is_none(),
            "a point 170 degrees around the globe from the sub-observer point is on the far side"
        );
        assert!(orthographic_project(0.0, 89.0, 0.0, 0.0).is_some());
        assert!(orthographic_project(0.0, 91.0, 0.0, 0.0).is_none());
    }

    #[test]
    fn orthographic_cos_c_is_one_at_centre_and_zero_at_the_limb() {
        let (_, _, centre) = orthographic_project(0.0, 0.0, 0.0, 0.0).unwrap();
        let (_, _, limb) = orthographic_project(0.0, 90.0, 0.0, 0.0).unwrap();
        assert!((centre - 1.0).abs() < 1e-9);
        assert!(limb.abs() < 1e-9);
    }

    #[test]
    fn dot_spacing_converges_towards_the_limb() {
        let radius = 500.0_f64;
        let screen_dx = |lng_a: f64, lng_b: f64| {
            let (xa, _, _) = orthographic_project(0.0, lng_a, 0.0, 0.0).unwrap();
            let (xb, _, _) = orthographic_project(0.0, lng_b, 0.0, 0.0).unwrap();
            ((xb - xa) * radius).abs()
        };
        let centre_gap = screen_dx(-2.5, 2.5);
        let edge_gap = screen_dx(77.5, 82.5);
        assert!(
            edge_gap < centre_gap * 0.9,
            "an orthographic globe's dots must converge towards the limb: a 5 degree step must \
             map to a measurably smaller screen gap near the edge than at the centre (not just \
             float noise) — centre={centre_gap:.3}px, edge={edge_gap:.3}px"
        );

        let flat_dx = |lng_a: f64, lng_b: f64| ((lng_b - lng_a) / 360.0 * 800.0).abs();
        assert_eq!(
            flat_dx(-2.5, 2.5),
            flat_dx(77.5, 82.5),
            "sanity: the flat equirectangular mapping this replaces has no such convergence"
        );
    }

    #[test]
    fn a_rotated_view_recentres_which_longitude_is_visible() {
        assert!(orthographic_project(0.0, 100.0, 0.0, 0.0).is_none());
        assert!(
            orthographic_project(0.0, 100.0, 0.0, 100.0).is_some(),
            "rotating the sub-observer longitude to 100 degrees must bring that meridian \
             into view"
        );
    }

    #[test]
    fn limb_shading_darkens_towards_the_edge_and_is_inert_when_unset() {
        assert_eq!(limb_shade(None, 0.0), 1.0);
        assert_eq!(limb_shade(Some(1.0), 1.0), 1.0);
        assert_eq!(limb_shade(Some(1.0), 0.0), 0.0);
        assert!((limb_shade(Some(0.5), 0.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn slerp_and_unit_conversion_round_trip_a_few_points() {
        for (lat, lng) in [(0.0, 0.0), (45.0, -30.0), (-60.0, 120.0), (10.0, 179.0)] {
            let v = latlng_to_unit(lat, lng);
            let (lat2, lng2) = unit_to_latlng(v);
            assert!((lat - lat2).abs() < 1e-6, "lat round trip: {lat} vs {lat2}");
            assert!((lng - lng2).abs() < 1e-6, "lng round trip: {lng} vs {lng2}");
        }
    }

    #[test]
    fn rotation_animates_linearly_from_start_at_to_the_fallback_duration() {
        let angle = RotationAngle::Animated {
            from: -20.0,
            to: 40.0,
            duration: None,
        };
        assert_eq!(angle.value_at(0.0, 0.0, 1.5), -20.0);
        assert_eq!(angle.value_at(1.5, 0.0, 1.5), 40.0);
        assert!((angle.value_at(0.75, 0.0, 1.5) - 10.0).abs() < 0.001);
    }

    #[test]
    fn an_orthographic_globe_paints_a_visibly_different_frame_from_the_flat_map() {
        let flat = dot_map_from(serde_json::json!({
            "points": [{"lat": 48.85, "lng": 2.35, "size": 20.0}],
            "show_world": false,
        }));
        let globe = dot_map_from(serde_json::json!({
            "projection": "orthographic",
            "points": [{"lat": 48.85, "lng": 2.35, "size": 20.0}],
            "show_world": false,
        }));
        let out_flat = render(&flat, 300, 300, 1.0);
        let out_globe = render(&globe, 300, 300, 1.0);
        assert_ne!(
            out_flat, out_globe,
            "an orthographic dot_map must not paint the flat equirectangular frame"
        );
    }

    fn has_orange_pixel(buf: &[u8]) -> bool {
        buf.as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[0] > 200 && p[1] > 90 && p[1] < 180 && p[2] < 60)
    }

    #[test]
    fn an_arc_on_the_near_side_is_visible_but_one_entirely_on_the_far_side_is_culled() {
        let visible = dot_map_from(serde_json::json!({
            "projection": "orthographic",
            "points": [],
            "show_world": false,
            "arcs": [{"from": [10.0, -30.0], "to": [10.0, 30.0], "draw_in": 1.0}],
        }));
        let hidden = dot_map_from(serde_json::json!({
            "projection": "orthographic",
            "points": [],
            "show_world": false,
            "arcs": [{"from": [10.0, 150.0], "to": [10.0, 170.0], "draw_in": 1.0}],
        }));
        let out_visible = render(&visible, 300, 300, 1.0);
        let out_hidden = render(&hidden, 300, 300, 1.0);
        assert!(
            has_orange_pixel(&out_visible),
            "an arc entirely on the near hemisphere must paint its (default orange) stroke"
        );
        assert!(
            !has_orange_pixel(&out_hidden),
            "an arc entirely on the far hemisphere must be culled, not drawn straight through \
             the globe"
        );
    }
}
