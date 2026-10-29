use std::sync::{
    Arc, RwLock,
    atomic::{AtomicU64, Ordering},
};

use bevy::prelude::Resource;

/// One painted canvas shared by the editor and the renderer.
///
/// The pixels are behind a lock so a stroke mutates them in place: the renderer
/// reads the buffer only when [`Self::revision`] moved, instead of the owner
/// cloning several megabytes every painted frame.
#[derive(Debug)]
pub struct CanvasPixels {
    pub width: u32,
    pub height: u32,
    pixels: RwLock<Vec<u8>>,
    revision: AtomicU64,
}

impl CanvasPixels {
    pub fn new(width: u32, height: u32, fill: [u8; 4]) -> Arc<Self> {
        let mut pixels = Vec::with_capacity((width as usize) * (height as usize) * 4);
        for _ in 0..(width as usize) * (height as usize) {
            pixels.extend_from_slice(&fill);
        }
        Arc::new(Self {
            width,
            height,
            pixels: RwLock::new(pixels),
            revision: AtomicU64::new(1),
        })
    }

    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    /// Mutate the canvas and publish the new revision. The closure receives
    /// tightly packed RGBA8 rows, top row first.
    pub fn edit(&self, edit: impl FnOnce(&mut [u8], u32, u32)) {
        let mut pixels = self
            .pixels
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        edit(&mut pixels, self.width, self.height);
        self.revision.fetch_add(1, Ordering::Release);
    }

    pub fn read(&self) -> std::sync::RwLockReadGuard<'_, Vec<u8>> {
        self.pixels
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn snapshot(&self) -> Vec<u8> {
        self.read().clone()
    }

    pub fn replace(&self, pixels: Vec<u8>) {
        debug_assert_eq!(pixels.len(), self.width as usize * self.height as usize * 4);
        let mut target = self
            .pixels
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *target = pixels;
        self.revision.fetch_add(1, Ordering::Release);
    }
}

/// The world-space plane that carries [`CanvasPixels`], in IW4 world units.
///
/// `right` and `up` are half-extent vectors, not unit axes, so a corner is
/// `origin ± right ± up`.
#[derive(Resource, Clone, Debug, Default)]
pub struct DrawCanvas {
    pub visible: bool,

    pub overlay: bool,
    pub pocket: bool,
    pub origin: [f32; 3],
    pub right: [f32; 3],
    pub up: [f32; 3],
    pub pixels: Option<Arc<CanvasPixels>>,
}

impl DrawCanvas {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn show(
        &mut self,
        origin: [f32; 3],
        right: [f32; 3],
        up: [f32; 3],
        pixels: Arc<CanvasPixels>,
    ) {
        self.visible = true;
        self.origin = origin;
        self.right = right;
        self.up = up;
        self.pixels = Some(pixels);
    }
}
