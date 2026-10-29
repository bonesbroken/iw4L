use std::sync::Arc;

use bevy::prelude::*;
use frame::CanvasPixels;
use render_scene::CameraView;

use crate::canvas;

pub const PAPER: [u8; 4] = [252, 252, 248, 255];
pub const MAX_BRUSH: u32 = 64;
pub const UNDO_LIMIT: usize = 16;

pub const PALETTE: [(&str, [u8; 4]); 8] = [
    ("Black", [24, 24, 24, 255]),
    ("Red", [214, 48, 44, 255]),
    ("Orange", [238, 134, 38, 255]),
    ("Yellow", [244, 210, 62, 255]),
    ("Green", [52, 166, 82, 255]),
    ("Blue", [46, 110, 224, 255]),
    ("Purple", [146, 78, 198, 255]),
    ("White", [255, 255, 255, 255]),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanvasAspect {
    Square,
    Widescreen,
    Portrait,
}

impl CanvasAspect {
    pub const ALL: [Self; 3] = [Self::Square, Self::Widescreen, Self::Portrait];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Square => "1:1",
            Self::Widescreen => "16:9",
            Self::Portrait => "9:16",
        }
    }

    pub const fn dimensions(self) -> (u32, u32) {
        match self {
            Self::Square => (1024, 1024),
            Self::Widescreen => (1024, 576),
            Self::Portrait => (576, 1024),
        }
    }

    pub const fn from_arg(value: &str) -> Option<Self> {
        if value.eq_ignore_ascii_case("square") || value.eq_ignore_ascii_case("1:1") {
            Some(Self::Square)
        } else if value.eq_ignore_ascii_case("wide")
            || value.eq_ignore_ascii_case("16:9")
            || value.eq_ignore_ascii_case("widescreen")
        {
            Some(Self::Widescreen)
        } else if value.eq_ignore_ascii_case("tall")
            || value.eq_ignore_ascii_case("9:16")
            || value.eq_ignore_ascii_case("portrait")
        {
            Some(Self::Portrait)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawTool {
    Brush,
    Line,
    Rect,
    Fill,
    Eraser,
    Picker,
}

impl DrawTool {
    pub const ALL: [Self; 6] = [
        Self::Brush,
        Self::Line,
        Self::Rect,
        Self::Fill,
        Self::Eraser,
        Self::Picker,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Brush => "Brush",
            Self::Line => "Line",
            Self::Rect => "Rect",
            Self::Fill => "Fill",
            Self::Eraser => "Eraser",
            Self::Picker => "Pick",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DrawAction {
    Tool(DrawTool),
    Aspect(CanvasAspect),
    Color(usize),
    Brush(i32),
    Undo,
    Redo,
    Clear,
    Save,
    Exit,
}

#[derive(Clone, Copy, Debug)]
pub struct Plane {
    pub origin: Vec3,

    pub right: Vec3,
    pub up: Vec3,
}

impl Plane {
    pub fn from_camera(camera: Transform, vertical_fov: f32, canvas_aspect: f32) -> Self {
        const DISTANCE: f32 = 190.0;
        const FILL: f32 = 0.42;
        let forward = *camera.forward();
        let horizontal = Vec3::new(forward.x, forward.y, 0.0);
        let forward = if horizontal.length_squared() > 1.0e-6 {
            horizontal.normalize()
        } else {
            forward
        };
        let normal = -forward;
        let mut right = Vec3::Z.cross(normal);
        if right.length_squared() < 1.0e-6 {
            right = *camera.right();
        }
        right = right.normalize();
        let up = normal.cross(right).normalize();
        let half_height = (vertical_fov * 0.5).tan() * DISTANCE * FILL;
        let half_width = half_height * canvas_aspect;
        Self {
            origin: camera.translation + forward * DISTANCE,
            right: right * half_width,
            up: up * half_height,
        }
    }

    pub fn normal(self) -> Vec3 {
        self.right.cross(self.up).normalize()
    }

    pub fn uv(self, eye: Vec3, direction: Vec3) -> Option<Vec2> {
        let normal = self.normal();
        let denominator = direction.dot(normal);
        if denominator.abs() < 1.0e-6 {
            return None;
        }
        let distance = (self.origin - eye).dot(normal) / denominator;
        if distance <= 0.0 {
            return None;
        }
        let hit = eye + direction * distance;
        let local = hit - self.origin;
        let u = 0.5 + local.dot(self.right) / (2.0 * self.right.length_squared());
        let v = 0.5 - local.dot(self.up) / (2.0 * self.up.length_squared());
        if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
            return None;
        }
        Some(Vec2::new(u, v))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SavedResources {
    pub hud: bool,
    pub pointer: bool,
    pub overlay: bool,
    pub viewmodel: bool,
    pub view_override: Option<CameraView>,
}

impl Default for SavedResources {
    fn default() -> Self {
        Self {
            hud: true,
            pointer: false,
            overlay: false,
            viewmodel: true,
            view_override: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Drag {
    pub erasing: bool,
    pub start: (i32, i32),
    pub last: (i32, i32),
    pub base: Option<Vec<u8>>,
}

#[derive(Resource)]
pub struct DrawState {
    pub active: bool,
    pub tool: DrawTool,
    pub color: [u8; 4],
    pub color_index: Option<usize>,
    pub brush: u32,
    pub status: String,

    pub preset: CanvasAspect,
    pub canvas: Option<Arc<CanvasPixels>>,
    pub plane: Option<Plane>,
    pub camera: Option<Transform>,
    pub vertical_fov: f32,
    pub aspect: f32,

    pub carrying: bool,
    pub pocket: bool,
    pub near_canvas: bool,
    pub carried_from: Option<Plane>,
    pub pocket_return: Option<Plane>,

    pub undo: Vec<Vec<u8>>,
    pub redo: Vec<Vec<u8>>,
    pub drag: Option<Drag>,
    pub cursor: Option<(u32, u32)>,

    pub actions: Vec<DrawAction>,
    pub ui_revision: u64,
    pub saved: SavedResources,
}

impl Default for DrawState {
    fn default() -> Self {
        Self {
            active: false,
            tool: DrawTool::Brush,
            color: PALETTE[0].1,
            color_index: Some(0),
            brush: 6,
            status: String::new(),
            preset: CanvasAspect::Widescreen,
            canvas: None,
            plane: None,
            camera: None,
            vertical_fov: 60.0_f32.to_radians(),
            aspect: 16.0 / 9.0,
            carrying: false,
            pocket: false,
            near_canvas: false,
            carried_from: None,
            pocket_return: None,
            undo: Vec::new(),
            redo: Vec::new(),
            drag: None,
            cursor: None,
            actions: Vec::new(),
            ui_revision: 1,
            saved: SavedResources::default(),
        }
    }
}

impl DrawState {
    pub fn canvas_aspect(&self) -> f32 {
        let (width, height) = self.preset.dimensions();
        width as f32 / height as f32
    }

    pub fn canvas(&mut self) -> Arc<CanvasPixels> {
        let (width, height) = self.preset.dimensions();
        self.canvas
            .get_or_insert_with(|| CanvasPixels::new(width, height, PAPER))
            .clone()
    }

    pub fn ensure_plane(&mut self, camera: Transform, vertical_fov: f32) {
        if self.plane.is_none() {
            self.plane = Some(Plane::from_camera(
                camera,
                vertical_fov,
                self.canvas_aspect(),
            ));
            self.camera = Some(camera);
            self.vertical_fov = vertical_fov;
        }
    }

    pub fn place_from_camera(&mut self, camera: Transform, vertical_fov: f32) {
        self.plane = Some(Plane::from_camera(
            camera,
            vertical_fov,
            self.canvas_aspect(),
        ));
        self.camera = Some(camera);
        self.vertical_fov = vertical_fov;
    }

    /// Put the editing eye in front of the board and aim it at the centre.
    pub fn focus_plane(&mut self, vertical_fov: f32) {
        let Some(plane) = self.plane else {
            return;
        };
        let normal = plane.normal();
        let origin = plane.origin + normal * 190.0;
        let right = plane.right.normalize();
        let up = plane.up.normalize();
        let rotation = Quat::from_mat3(&Mat3::from_cols(right, up, normal));
        self.camera = Some(Transform {
            translation: origin,
            rotation,
            scale: Vec3::ONE,
        });
        self.vertical_fov = vertical_fov;
    }

    pub fn place_pocket(&mut self, camera: Transform, vertical_fov: f32) {
        const DISTANCE: f32 = 80.0;
        const RIGHT: f32 = 15.0;
        const DOWN: f32 = 11.0;
        const HALF_HEIGHT: f32 = 5.0;
        let half_width = HALF_HEIGHT * self.canvas_aspect();
        self.plane = Some(Plane {
            origin: camera.translation
                + *camera.forward() * DISTANCE
                + *camera.right() * RIGHT
                + *camera.up() * -DOWN,
            right: *camera.right() * half_width,
            up: *camera.up() * HALF_HEIGHT,
        });
        self.camera = Some(camera);
        self.vertical_fov = vertical_fov;
    }

    pub fn enter_pocket(&mut self, camera: Transform, vertical_fov: f32) {
        self.canvas();
        self.pocket_return = self.plane;
        self.pocket = true;
        self.active = false;
        self.carrying = false;
        self.drag = None;
        self.place_pocket(camera, vertical_fov);
        self.status = "Pocket canvas".into();
        self.ui_revision = self.ui_revision.wrapping_add(1);
    }

    pub fn exit_pocket(&mut self) {
        self.pocket = false;
        if let Some(plane) = self.pocket_return.take() {
            self.plane = Some(plane);
        } else if let Some(camera) = self.camera {
            self.plane = Some(Plane::from_camera(
                camera,
                self.vertical_fov,
                self.canvas_aspect(),
            ));
        }
        self.status = "Pocket canvas away".into();
        self.ui_revision = self.ui_revision.wrapping_add(1);
    }

    pub fn set_aspect(&mut self, preset: CanvasAspect) {
        if self.preset == preset {
            return;
        }
        let old = self.canvas.clone();
        let (width, height) = preset.dimensions();
        let next = CanvasPixels::new(width, height, PAPER);
        if let Some(old) = old {
            let previous = old.snapshot();
            let (old_width, old_height) = (old.width, old.height);
            next.edit(|pixels, width, height| {
                canvas::fit_into(&previous, old_width, old_height, pixels, width, height);
            });
        }
        self.preset = preset;
        self.canvas = Some(next);
        self.undo.clear();
        self.redo.clear();
        self.drag = None;
        if let Some(camera) = self.camera {
            self.plane = Some(Plane::from_camera(
                camera,
                self.vertical_fov,
                self.canvas_aspect(),
            ));
        }
        self.status = format!("Canvas {}", preset.label());
        self.ui_revision = self.ui_revision.wrapping_add(1);
    }

    pub fn new_canvas(&mut self) {
        let (width, height) = self.preset.dimensions();
        self.canvas = Some(CanvasPixels::new(width, height, PAPER));
        self.undo.clear();
        self.redo.clear();
        self.drag = None;
        self.ui_revision = self.ui_revision.wrapping_add(1);
    }

    pub fn start_carry(&mut self, camera: Transform, vertical_fov: f32) {
        self.carried_from = self.plane;
        self.carrying = true;
        self.active = false;
        self.drag = None;
        self.follow_camera(camera, vertical_fov);
        self.status = "Move the canvas · use places it".into();
        self.ui_revision = self.ui_revision.wrapping_add(1);
    }

    pub fn follow_camera(&mut self, camera: Transform, vertical_fov: f32) {
        self.plane = Some(Plane::from_camera(
            camera,
            vertical_fov,
            self.canvas_aspect(),
        ));
        self.camera = Some(camera);
        self.vertical_fov = vertical_fov;
    }

    pub fn place_carry(&mut self) {
        self.carrying = false;
        self.carried_from = None;
        self.status = "Canvas placed".into();
        self.ui_revision = self.ui_revision.wrapping_add(1);
    }

    pub fn push_undo(&mut self) {
        let Some(canvas) = self.canvas.as_ref() else {
            return;
        };
        if self.undo.len() >= UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.undo.push(canvas.snapshot());
        self.redo.clear();
        self.ui_revision = self.ui_revision.wrapping_add(1);
    }

    pub fn undo(&mut self) {
        let Some(canvas) = self.canvas.clone() else {
            return;
        };
        let Some(previous) = self.undo.pop() else {
            self.status = "Nothing to undo".into();
            return;
        };
        self.redo.push(canvas.snapshot());
        canvas.replace(previous);
        self.ui_revision = self.ui_revision.wrapping_add(1);
    }

    pub fn redo(&mut self) {
        let Some(canvas) = self.canvas.clone() else {
            return;
        };
        let Some(next) = self.redo.pop() else {
            self.status = "Nothing to redo".into();
            return;
        };
        self.undo.push(canvas.snapshot());
        canvas.replace(next);
        self.ui_revision = self.ui_revision.wrapping_add(1);
    }

    pub fn clear(&mut self) {
        let Some(canvas) = self.canvas.clone() else {
            return;
        };
        self.push_undo();
        canvas.edit(|pixels, _, _| canvas::fill(pixels, PAPER));
        self.status = "Canvas cleared".into();
        self.ui_revision = self.ui_revision.wrapping_add(1);
    }

    pub fn set_tool(&mut self, tool: DrawTool) {
        self.tool = tool;
        self.drag = None;
        self.ui_revision = self.ui_revision.wrapping_add(1);
    }

    pub fn set_color(&mut self, index: usize) {
        if let Some((_, color)) = PALETTE.get(index) {
            self.color = *color;
            self.color_index = Some(index);
            self.ui_revision = self.ui_revision.wrapping_add(1);
        }
    }

    pub fn set_custom_color(&mut self, color: [u8; 4]) {
        self.color = color;
        self.color_index = PALETTE.iter().position(|(_, entry)| *entry == color);
        self.ui_revision = self.ui_revision.wrapping_add(1);
    }

    pub fn set_brush(&mut self, brush: u32) {
        self.brush = brush.clamp(1, MAX_BRUSH);
        self.ui_revision = self.ui_revision.wrapping_add(1);
    }

    pub fn nudge_brush(&mut self, delta: i32) {
        let brush = (self.brush as i32 + delta).clamp(1, MAX_BRUSH as i32);
        self.set_brush(brush as u32);
    }

    pub fn reset(&mut self) {
        *self = Self {
            saved: self.saved,
            ..Self::default()
        };
    }
}
