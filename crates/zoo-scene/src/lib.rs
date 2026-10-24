//! Zoo-specific scene and camera interaction policy.
//!
//! Generic view/projection math remains owned by `three-d-camera`, and matrix inversion /
//! unprojection by `three-d-projective`. This crate owns only the management-game policy needed
//! to frame and manipulate a Zoo park view: orbit and tilt bounds, zoom limits, pan clamping,
//! park framing, and the mapping from a picked ground point to Zoo's park grid.
//!
//! World convention: park tile `(x, y)` covers the ground square `[x, x + 1] × [y, y + 1]` on the
//! `y = 0` plane, so its center is `(x + 0.5, 0, y + 0.5)`. World `+y` is up.

use core::fmt;
use serde::Serialize;
use three_d_camera::{CameraError, PerspectiveCamera};
use three_d_core::Vec3;
use three_d_projective::{ProjectiveError, untransform_point_projective_f64};
use wasm_bindgen::prelude::*;

const DEFAULT_YAW_DEGREES: f32 = -38.0;
const DEFAULT_PITCH_DEGREES: f32 = 46.0;
const MIN_PITCH_DEGREES: f32 = 22.0;
const MAX_PITCH_DEGREES: f32 = 82.0;
const MIN_ZOOM: f32 = 0.6;
const MAX_ZOOM: f32 = 7.0;
const FIELD_OF_VIEW_Y_DEGREES: f32 = 36.0;
/// Fraction of the clip volume the framed park may occupy at zoom 1.
const FRAMING_EXTENT: f32 = 0.97;
/// Height of the tallest park structures that framing keeps in view.
const FRAMING_HEIGHT: f32 = 1.4;
const FRAMING_SAMPLES: usize = 24;
const FRAMING_ITERATIONS: usize = 32;
/// How far (in tiles) the orbit target may be dragged past the park edge.
const PAN_MARGIN: f32 = 3.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParkCameraError {
    InvalidParkExtent,
    InvalidAspect,
    InvalidZoomFactor,
    InvalidScreenPoint,
    SharedCamera(CameraError),
    SharedProjective(ProjectiveError),
}

impl fmt::Display for ParkCameraError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidParkExtent => {
                formatter.write_str("park width and depth must be finite and positive")
            }
            Self::InvalidAspect => {
                formatter.write_str("viewport aspect must be finite and positive")
            }
            Self::InvalidZoomFactor => {
                formatter.write_str("zoom factor must be finite and positive")
            }
            Self::InvalidScreenPoint => {
                formatter.write_str("normalized screen coordinates must be finite")
            }
            Self::SharedCamera(error) => write!(formatter, "shared camera rejected view: {error}"),
            Self::SharedProjective(error) => {
                write!(formatter, "shared projective math rejected pick: {error}")
            }
        }
    }
}

impl std::error::Error for ParkCameraError {}

impl From<CameraError> for ParkCameraError {
    fn from(value: CameraError) -> Self {
        Self::SharedCamera(value)
    }
}

impl From<ProjectiveError> for ParkCameraError {
    fn from(value: ProjectiveError) -> Self {
        Self::SharedProjective(value)
    }
}

/// A normalized device coordinate on the viewport: `x` right and `y` up, both in `[-1, 1]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenPoint {
    pub x: f32,
    pub y: f32,
}

impl ScreenPoint {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    fn validate(self) -> Result<Self, ParkCameraError> {
        if self.x.is_finite() && self.y.is_finite() {
            Ok(self)
        } else {
            Err(ParkCameraError::InvalidScreenPoint)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ParkTile {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParkCameraRig {
    target: Vec3,
    park_width: f32,
    park_depth: f32,
    yaw_degrees: f32,
    pitch_degrees: f32,
    zoom: f32,
}

impl ParkCameraRig {
    pub fn new(park_width: f32, park_depth: f32) -> Result<Self, ParkCameraError> {
        if !park_width.is_finite()
            || !park_depth.is_finite()
            || park_width <= 0.0
            || park_depth <= 0.0
        {
            return Err(ParkCameraError::InvalidParkExtent);
        }

        Ok(Self {
            target: Self::park_center(park_width, park_depth),
            park_width,
            park_depth,
            yaw_degrees: DEFAULT_YAW_DEGREES.rem_euclid(360.0),
            pitch_degrees: DEFAULT_PITCH_DEGREES,
            zoom: 1.0,
        })
    }

    fn park_center(park_width: f32, park_depth: f32) -> Vec3 {
        Vec3::new(park_width * 0.5, 0.0, park_depth * 0.5)
    }

    pub fn target(self) -> Vec3 {
        self.target
    }

    pub fn yaw_degrees(self) -> f32 {
        self.yaw_degrees
    }

    pub fn pitch_degrees(self) -> f32 {
        self.pitch_degrees
    }

    pub fn zoom(self) -> f32 {
        self.zoom
    }

    pub fn reset(&mut self) {
        *self = Self {
            target: Self::park_center(self.park_width, self.park_depth),
            yaw_degrees: DEFAULT_YAW_DEGREES.rem_euclid(360.0),
            pitch_degrees: DEFAULT_PITCH_DEGREES,
            zoom: 1.0,
            ..*self
        };
    }

    /// Orbits around the current target. Yaw wraps freely; pitch stays inside the management
    /// view bounds so the ground plane is always visible and never viewed from below.
    pub fn orbit_by_degrees(&mut self, yaw_delta: f32, pitch_delta: f32) {
        if yaw_delta.is_finite() {
            self.yaw_degrees = (self.yaw_degrees + yaw_delta).rem_euclid(360.0);
        }
        if pitch_delta.is_finite() {
            self.pitch_degrees =
                (self.pitch_degrees + pitch_delta).clamp(MIN_PITCH_DEGREES, MAX_PITCH_DEGREES);
        }
    }

    /// Zooms by `factor` while keeping the ground point under `anchor` fixed on screen.
    pub fn zoom_at(
        &mut self,
        factor: f32,
        anchor: ScreenPoint,
        aspect: f32,
    ) -> Result<(), ParkCameraError> {
        if !factor.is_finite() || factor <= 0.0 {
            return Err(ParkCameraError::InvalidZoomFactor);
        }
        let anchor = anchor.validate()?;
        let before = self.pick_ground(anchor, aspect)?;
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let after = self.pick_ground(anchor, aspect)?;
        if let (Some(before), Some(after)) = (before, after) {
            self.shift_target(before.x - after.x, before.z - after.z);
        }
        Ok(())
    }

    /// Drags the ground so the point under `from` ends up under `to` ("grab the map" panning).
    pub fn pan_drag(
        &mut self,
        from: ScreenPoint,
        to: ScreenPoint,
        aspect: f32,
    ) -> Result<(), ParkCameraError> {
        let from = self.pick_ground(from.validate()?, aspect)?;
        let to = self.pick_ground(to.validate()?, aspect)?;
        if let (Some(from), Some(to)) = (from, to) {
            self.shift_target(from.x - to.x, from.z - to.z);
        }
        Ok(())
    }

    /// Opens the default view centered on park tile `(x, y)` at `zoom`, framing a scenario's
    /// first objective. Orbit, tilt, zoom and pan stay free afterwards, and [`Self::reset`]
    /// still frames the whole park.
    pub fn focus_tile(&mut self, x: f32, y: f32, zoom: f32) {
        self.reset();
        if x.is_finite() && y.is_finite() {
            self.target.x = (x + 0.5).clamp(0.0, self.park_width);
            self.target.z = (y + 0.5).clamp(0.0, self.park_depth);
        }
        if zoom.is_finite() {
            self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        }
    }

    fn shift_target(&mut self, delta_x: f32, delta_z: f32) {
        self.target.x = (self.target.x + delta_x).clamp(-PAN_MARGIN, self.park_width + PAN_MARGIN);
        self.target.z = (self.target.z + delta_z).clamp(-PAN_MARGIN, self.park_depth + PAN_MARGIN);
    }

    fn eye_at_distance(self, distance: f32) -> Vec3 {
        let pitch = self.pitch_degrees.to_radians();
        let yaw = self.yaw_degrees.to_radians();
        let horizontal = distance * pitch.cos();
        Vec3::new(
            self.target.x + horizontal * yaw.sin(),
            self.target.y + distance * pitch.sin(),
            self.target.z + horizontal * yaw.cos(),
        )
    }

    fn park_span(self) -> f32 {
        self.park_width.max(self.park_depth)
    }

    fn perspective_at_distance(
        self,
        distance: f32,
        aspect: f32,
    ) -> Result<PerspectiveCamera, ParkCameraError> {
        let span = self.park_span();
        Ok(PerspectiveCamera::new(
            self.eye_at_distance(distance),
            self.target,
            Vec3::new(0.0, 1.0, 0.0),
            FIELD_OF_VIEW_Y_DEGREES.to_radians(),
            aspect,
            (distance * 0.04).max(0.05),
            distance * 2.0 + span * 8.0,
        )?)
    }

    /// Framing is Zoo policy: find the closest distance at which the park's circumscribed circle
    /// (lifted to the height of park buildings) fits inside the shared perspective frustum. Using
    /// the circle keeps the fit independent of yaw, so orbiting never makes the park "breathe".
    fn fit_distance(self, aspect: f32) -> Result<f32, ParkCameraError> {
        let framing = Self {
            target: Self::park_center(self.park_width, self.park_depth),
            ..self
        };
        let radius = 0.5 * self.park_width.hypot(self.park_depth);
        let fits = |distance: f32| -> Result<bool, ParkCameraError> {
            let camera = framing.perspective_at_distance(distance, aspect)?;
            let matrix = camera.view_projection_matrix();
            for sample in 0..FRAMING_SAMPLES {
                let angle = sample as f32 / FRAMING_SAMPLES as f32 * core::f32::consts::TAU;
                for height in [0.0, FRAMING_HEIGHT] {
                    let point = Vec3::new(
                        framing.target.x + radius * angle.cos(),
                        height,
                        framing.target.z + radius * angle.sin(),
                    );
                    let clip = three_d_projective::transform_point_projective(matrix, point)?;
                    if clip.x.abs() > FRAMING_EXTENT || clip.y.abs() > FRAMING_EXTENT {
                        return Ok(false);
                    }
                }
            }
            Ok(true)
        };

        let mut near = radius * 0.5;
        let mut far = radius * 12.0;
        if !fits(far)? {
            return Ok(far);
        }
        for _ in 0..FRAMING_ITERATIONS {
            let middle = (near + far) * 0.5;
            if fits(middle)? {
                far = middle;
            } else {
                near = middle;
            }
        }
        Ok(far)
    }

    pub fn distance(self, aspect: f32) -> Result<f32, ParkCameraError> {
        validate_aspect(aspect)?;
        Ok(self.fit_distance(aspect)? / self.zoom)
    }

    pub fn camera(self, aspect: f32) -> Result<PerspectiveCamera, ParkCameraError> {
        let distance = self.distance(aspect)?;
        self.perspective_at_distance(distance, aspect)
    }

    /// Casts a ray through `point` using the shared inverse view-projection and intersects it
    /// with the park ground plane. Returns `None` when the ray does not reach the ground.
    pub fn pick_ground(
        self,
        point: ScreenPoint,
        aspect: f32,
    ) -> Result<Option<Vec3>, ParkCameraError> {
        let point = point.validate()?;
        let matrix = self.camera(aspect)?.view_projection_matrix();
        let x = f64::from(point.x);
        let y = f64::from(point.y);
        let near = untransform_point_projective_f64(matrix, [x, y, 0.0])?;
        let far = untransform_point_projective_f64(matrix, [x, y, 1.0])?;
        let direction_y = far[1] - near[1];
        if direction_y >= -f64::EPSILON {
            return Ok(None);
        }
        let t = -near[1] / direction_y;
        if !(0.0..=1.0).contains(&t) {
            return Ok(None);
        }
        Ok(Some(Vec3::new(
            (near[0] + (far[0] - near[0]) * t) as f32,
            0.0,
            (near[2] + (far[2] - near[2]) * t) as f32,
        )))
    }

    /// Maps a picked ground point to the park grid. Park-grid semantics are Zoo-owned.
    pub fn pick_tile(
        self,
        point: ScreenPoint,
        aspect: f32,
    ) -> Result<Option<ParkTile>, ParkCameraError> {
        Ok(self
            .pick_ground(point, aspect)?
            .and_then(|ground| self.tile_at(ground)))
    }

    fn tile_at(self, ground: Vec3) -> Option<ParkTile> {
        let x = ground.x.floor();
        let y = ground.z.floor();
        if x < 0.0 || y < 0.0 || x >= self.park_width || y >= self.park_depth {
            return None;
        }
        Some(ParkTile {
            x: x as i32,
            y: y as i32,
        })
    }
}

fn validate_aspect(aspect: f32) -> Result<(), ParkCameraError> {
    if aspect.is_finite() && aspect > 0.0 {
        Ok(())
    } else {
        Err(ParkCameraError::InvalidAspect)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BrowserCameraFrame {
    view_matrix: [f32; 16],
    projection_matrix: [f32; 16],
    eye: [f32; 3],
    target: [f32; 3],
    yaw_degrees: f32,
    pitch_degrees: f32,
    zoom: f32,
}

impl BrowserCameraFrame {
    fn from_rig(rig: ParkCameraRig, aspect: f32) -> Result<Self, ParkCameraError> {
        let camera = rig.camera(aspect)?;
        Ok(Self {
            view_matrix: camera.view_matrix().elements,
            projection_matrix: camera.projection_matrix().elements,
            eye: [camera.eye.x, camera.eye.y, camera.eye.z],
            target: [camera.target.x, camera.target.y, camera.target.z],
            yaw_degrees: rig.yaw_degrees(),
            pitch_degrees: rig.pitch_degrees(),
            zoom: rig.zoom(),
        })
    }
}

#[derive(Serialize)]
struct BrowserPick {
    ground: [f32; 2],
    tile: Option<ParkTile>,
}

fn js_error(error: impl fmt::Display) -> JsValue {
    JsValue::from_str(&error.to_string())
}

#[wasm_bindgen]
pub struct ParkCameraBridge {
    rig: ParkCameraRig,
}

#[wasm_bindgen]
impl ParkCameraBridge {
    #[wasm_bindgen(constructor)]
    pub fn new(park_width: f32, park_depth: f32) -> Result<ParkCameraBridge, JsValue> {
        Ok(Self {
            rig: ParkCameraRig::new(park_width, park_depth).map_err(js_error)?,
        })
    }

    pub fn reset(&mut self) {
        self.rig.reset();
    }

    pub fn focus_tile(&mut self, x: f32, y: f32, zoom: f32) {
        self.rig.focus_tile(x, y, zoom);
    }

    pub fn orbit_by_degrees(&mut self, yaw_delta: f32, pitch_delta: f32) {
        self.rig.orbit_by_degrees(yaw_delta, pitch_delta);
    }

    pub fn zoom_at(
        &mut self,
        factor: f32,
        ndc_x: f32,
        ndc_y: f32,
        aspect: f32,
    ) -> Result<(), JsValue> {
        self.rig
            .zoom_at(factor, ScreenPoint::new(ndc_x, ndc_y), aspect)
            .map_err(js_error)
    }

    pub fn pan_drag(
        &mut self,
        from_x: f32,
        from_y: f32,
        to_x: f32,
        to_y: f32,
        aspect: f32,
    ) -> Result<(), JsValue> {
        self.rig
            .pan_drag(
                ScreenPoint::new(from_x, from_y),
                ScreenPoint::new(to_x, to_y),
                aspect,
            )
            .map_err(js_error)
    }

    pub fn frame_json(&self, aspect: f32) -> Result<String, JsValue> {
        let frame = BrowserCameraFrame::from_rig(self.rig, aspect).map_err(js_error)?;
        serde_json::to_string(&frame).map_err(js_error)
    }

    /// Returns `null` when the ray misses the ground, otherwise the ground point and the park
    /// tile under it (`tile` is `null` outside the park).
    pub fn pick_json(&self, ndc_x: f32, ndc_y: f32, aspect: f32) -> Result<String, JsValue> {
        let ground = self
            .rig
            .pick_ground(ScreenPoint::new(ndc_x, ndc_y), aspect)
            .map_err(js_error)?;
        let pick = ground.map(|ground| BrowserPick {
            ground: [ground.x, ground.z],
            tile: self.rig.tile_at(ground),
        });
        serde_json::to_string(&pick).map_err(js_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDTH: f32 = 20.0;
    const DEPTH: f32 = 14.0;
    const DESKTOP: f32 = 1440.0 / 780.0;
    const PHONE: f32 = 390.0 / 560.0;

    fn rig() -> ParkCameraRig {
        ParkCameraRig::new(WIDTH, DEPTH).expect("park extent is valid")
    }

    fn project(rig: ParkCameraRig, aspect: f32, point: Vec3) -> Vec3 {
        let matrix = rig.camera(aspect).expect("camera").view_projection_matrix();
        three_d_projective::transform_point_projective(matrix, point).expect("projects")
    }

    fn assert_park_inside_frame(rig: ParkCameraRig, aspect: f32) {
        for x in [0.0, WIDTH] {
            for z in [0.0, DEPTH] {
                for y in [0.0, FRAMING_HEIGHT] {
                    let clip = project(rig, aspect, Vec3::new(x, y, z));
                    assert!(
                        clip.x.abs() <= 1.0 && clip.y.abs() <= 1.0 && (0.0..=1.0).contains(&clip.z),
                        "park corner ({x}, {y}, {z}) escaped frame at yaw {} pitch {} aspect {aspect}: {clip:?}",
                        rig.yaw_degrees(),
                        rig.pitch_degrees(),
                    );
                }
            }
        }
    }

    #[test]
    fn rejects_invalid_park_extent() {
        assert_eq!(
            ParkCameraRig::new(0.0, DEPTH),
            Err(ParkCameraError::InvalidParkExtent)
        );
        assert_eq!(
            ParkCameraRig::new(WIDTH, f32::NAN),
            Err(ParkCameraError::InvalidParkExtent)
        );
    }

    #[test]
    fn default_view_targets_park_center() {
        assert_eq!(rig().target(), Vec3::new(10.0, 0.0, 7.0));
    }

    #[test]
    fn focus_frames_the_objective_tile_and_reset_returns_to_the_whole_park() {
        for aspect in [DESKTOP, PHONE] {
            let mut rig = rig();
            rig.orbit_by_degrees(40.0, 20.0);
            rig.focus_tile(15.0, 3.0, 1.8);
            assert_eq!(rig.target(), Vec3::new(15.5, 0.0, 3.5));
            let center = rig
                .pick_tile(ScreenPoint::new(0.0, 0.0), aspect)
                .expect("picks")
                .expect("hits the park");
            assert_eq!(
                center,
                ParkTile { x: 15, y: 3 },
                "the focused tile is under the center"
            );
            rig.focus_tile(99.0, -5.0, 100.0);
            assert_eq!(rig.target(), Vec3::new(WIDTH, 0.0, 0.0));
            assert_eq!(rig.zoom(), MAX_ZOOM);
            rig.reset();
            assert_park_inside_frame(rig, aspect);
        }
    }

    #[test]
    fn full_park_stays_in_frame_through_full_orbit_at_both_tilt_bounds() {
        for aspect in [DESKTOP, PHONE] {
            for pitch_extreme in [-200.0, 200.0] {
                let mut rig = rig();
                rig.orbit_by_degrees(0.0, pitch_extreme);
                for _ in 0..36 {
                    assert_park_inside_frame(rig, aspect);
                    rig.orbit_by_degrees(10.0, 0.0);
                }
            }
        }
    }

    #[test]
    fn orbit_wraps_yaw_and_clamps_pitch() {
        let mut rig = rig();
        let initial_yaw = rig.yaw_degrees();
        rig.orbit_by_degrees(720.0, 0.0);
        assert!((rig.yaw_degrees() - initial_yaw.rem_euclid(360.0)).abs() < 1.0e-3);

        rig.orbit_by_degrees(0.0, 500.0);
        assert_eq!(rig.pitch_degrees(), MAX_PITCH_DEGREES);
        rig.orbit_by_degrees(0.0, -500.0);
        assert_eq!(rig.pitch_degrees(), MIN_PITCH_DEGREES);
    }

    #[test]
    fn orbit_keeps_target_fixed_and_moves_eye() {
        let mut rig = rig();
        let before = rig.camera(DESKTOP).expect("camera");
        rig.orbit_by_degrees(90.0, 0.0);
        let after = rig.camera(DESKTOP).expect("camera");
        assert_eq!(before.target, after.target);
        assert!((before.eye.x - after.eye.x).abs() > 1.0);
    }

    #[test]
    fn picking_the_screen_center_hits_the_target() {
        let rig = rig();
        let ground = rig
            .pick_ground(ScreenPoint::new(0.0, 0.0), DESKTOP)
            .expect("pick")
            .expect("ray reaches ground");
        assert!((ground.x - 10.0).abs() < 1.0e-3 && (ground.z - 7.0).abs() < 1.0e-3);
        assert_eq!(
            rig.pick_tile(ScreenPoint::new(0.0, 0.0), DESKTOP),
            Ok(Some(ParkTile { x: 10, y: 7 }))
        );
    }

    #[test]
    fn picking_round_trips_projected_tile_centers() {
        let mut rig = rig();
        rig.orbit_by_degrees(133.0, 12.0);
        rig.zoom_at(1.7, ScreenPoint::new(0.2, -0.1), DESKTOP)
            .expect("zoom");
        for (x, y) in [(0, 0), (3, 9), (19, 13), (12, 2)] {
            let center = Vec3::new(x as f32 + 0.5, 0.0, y as f32 + 0.5);
            let clip = project(rig, DESKTOP, center);
            if clip.x.abs() > 1.0 || clip.y.abs() > 1.0 {
                continue;
            }
            assert_eq!(
                rig.pick_tile(ScreenPoint::new(clip.x, clip.y), DESKTOP),
                Ok(Some(ParkTile { x, y }))
            );
        }
    }

    #[test]
    fn picking_outside_park_reports_no_tile() {
        let rig = rig();
        let corner = project(rig, DESKTOP, Vec3::new(-1.5, 0.0, -1.5));
        assert_eq!(
            rig.pick_tile(ScreenPoint::new(corner.x, corner.y), DESKTOP),
            Ok(None)
        );
    }

    #[test]
    fn pan_drag_moves_grabbed_ground_under_cursor() {
        let mut rig = rig();
        let from = ScreenPoint::new(-0.2, 0.1);
        let to = ScreenPoint::new(0.1, -0.2);
        let grabbed = rig.pick_ground(from, DESKTOP).expect("pick").expect("hit");
        rig.pan_drag(from, to, DESKTOP).expect("pan");
        let under_cursor = rig.pick_ground(to, DESKTOP).expect("pick").expect("hit");
        assert!((grabbed.x - under_cursor.x).abs() < 1.0e-3);
        assert!((grabbed.z - under_cursor.z).abs() < 1.0e-3);
    }

    #[test]
    fn pan_is_clamped_near_the_park() {
        let mut rig = rig();
        for _ in 0..200 {
            rig.pan_drag(
                ScreenPoint::new(0.6, 0.0),
                ScreenPoint::new(-0.6, 0.0),
                DESKTOP,
            )
            .expect("pan");
        }
        let target = rig.target();
        assert!(target.x >= -PAN_MARGIN && target.x <= WIDTH + PAN_MARGIN);
        assert!(target.z >= -PAN_MARGIN && target.z <= DEPTH + PAN_MARGIN);
    }

    #[test]
    fn zoom_keeps_anchor_fixed_and_is_clamped() {
        let mut rig = rig();
        let anchor = ScreenPoint::new(0.35, -0.25);
        let before = rig
            .pick_ground(anchor, DESKTOP)
            .expect("pick")
            .expect("hit");
        let distance_before = rig.distance(DESKTOP).expect("distance");
        rig.zoom_at(2.0, anchor, DESKTOP).expect("zoom");
        let after = rig
            .pick_ground(anchor, DESKTOP)
            .expect("pick")
            .expect("hit");
        assert!(rig.distance(DESKTOP).expect("distance") < distance_before);
        assert!((before.x - after.x).abs() < 1.0e-2 && (before.z - after.z).abs() < 1.0e-2);

        rig.zoom_at(1000.0, anchor, DESKTOP).expect("zoom");
        assert_eq!(rig.zoom(), MAX_ZOOM);
        rig.zoom_at(1.0e-6, anchor, DESKTOP).expect("zoom");
        assert_eq!(rig.zoom(), MIN_ZOOM);
    }

    #[test]
    fn reset_restores_default_view() {
        let mut rig = rig();
        rig.orbit_by_degrees(77.0, -10.0);
        rig.zoom_at(3.0, ScreenPoint::new(0.5, 0.5), DESKTOP)
            .expect("zoom");
        rig.reset();
        assert_eq!(rig, self::rig());
    }

    #[test]
    fn rejects_invalid_inputs() {
        let mut rig = rig();
        assert_eq!(rig.camera(0.0).err(), Some(ParkCameraError::InvalidAspect));
        assert_eq!(
            rig.zoom_at(f32::INFINITY, ScreenPoint::new(0.0, 0.0), DESKTOP),
            Err(ParkCameraError::InvalidZoomFactor)
        );
        assert_eq!(
            rig.pick_ground(ScreenPoint::new(f32::NAN, 0.0), DESKTOP),
            Err(ParkCameraError::InvalidScreenPoint)
        );
    }

    #[test]
    fn browser_frame_uses_shared_camera_matrices() {
        let rig = rig();
        let shared = rig.camera(DESKTOP).expect("shared camera is valid");
        let frame = BrowserCameraFrame::from_rig(rig, DESKTOP).expect("frame is valid");

        assert_eq!(frame.view_matrix, shared.view_matrix().elements);
        assert_eq!(frame.projection_matrix, shared.projection_matrix().elements);
        assert_eq!(frame.yaw_degrees, DEFAULT_YAW_DEGREES.rem_euclid(360.0));
        assert_eq!(frame.zoom, 1.0);
    }
}
