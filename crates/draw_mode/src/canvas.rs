use std::{
    fs::File,
    io::BufWriter,
    path::{Component, Path, PathBuf},
};

use crate::state::PAPER;

pub fn fill(pixels: &mut [u8], color: [u8; 4]) {
    for chunk in pixels.as_chunks_mut::<4>().0 {
        chunk.copy_from_slice(&color);
    }
}

pub fn put_pixel(pixels: &mut [u8], width: u32, height: u32, x: i32, y: i32, color: [u8; 4]) {
    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
        return;
    }
    let index = ((y as u32 * width + x as u32) * 4) as usize;
    pixels[index..index + 4].copy_from_slice(&color);
}

pub fn pixel(pixels: &[u8], width: u32, height: u32, x: i32, y: i32) -> Option<[u8; 4]> {
    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
        return None;
    }
    let index = ((y as u32 * width + x as u32) * 4) as usize;
    pixels[index..index + 4].try_into().ok()
}

pub fn stamp(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    center: (i32, i32),
    brush: u32,
    color: [u8; 4],
) {
    let radius = (brush as f32 * 0.5).max(0.5);
    let reach = radius.ceil() as i32;
    let radius_squared = radius * radius;
    for dy in -reach..=reach {
        for dx in -reach..=reach {
            if (dx * dx + dy * dy) as f32 <= radius_squared {
                put_pixel(pixels, width, height, center.0 + dx, center.1 + dy, color);
            }
        }
    }
}

pub fn line(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    from: (i32, i32),
    to: (i32, i32),
    brush: u32,
    color: [u8; 4],
) {
    let (mut x, mut y) = from;
    let (x1, y1) = to;
    let dx = (x1 - x).abs();
    let sx = if x < x1 { 1 } else { -1 };
    let dy = -(y1 - y).abs();
    let sy = if y < y1 { 1 } else { -1 };
    let mut error = dx + dy;
    loop {
        stamp(pixels, width, height, (x, y), brush, color);
        if x == x1 && y == y1 {
            break;
        }
        let doubled = error * 2;
        if doubled >= dy {
            error += dy;
            x += sx;
        }
        if doubled <= dx {
            error += dx;
            y += sy;
        }
    }
}

pub fn rectangle(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    start: (i32, i32),
    end: (i32, i32),
    brush: u32,
    color: [u8; 4],
) {
    let left = start.0.min(end.0);
    let right = start.0.max(end.0);
    let top = start.1.min(end.1);
    let bottom = start.1.max(end.1);
    line(
        pixels,
        width,
        height,
        (left, top),
        (right, top),
        brush,
        color,
    );
    line(
        pixels,
        width,
        height,
        (right, top),
        (right, bottom),
        brush,
        color,
    );
    line(
        pixels,
        width,
        height,
        (right, bottom),
        (left, bottom),
        brush,
        color,
    );
    line(
        pixels,
        width,
        height,
        (left, bottom),
        (left, top),
        brush,
        color,
    );
}

pub fn flood_fill(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    seed: (i32, i32),
    color: [u8; 4],
) -> bool {
    let Some(target) = pixel(pixels, width, height, seed.0, seed.1) else {
        return false;
    };
    if target == color {
        return false;
    }
    let width_i = width as i32;
    let height_i = height as i32;
    let mut stack = vec![seed];
    while let Some((x, y)) = stack.pop() {
        if y < 0 || y >= height_i {
            continue;
        }
        let mut left = x;
        while left >= 0
            && pixel(pixels, width, height, left, y).is_some_and(|value| value == target)
        {
            left -= 1;
        }
        left += 1;
        let mut right = x;
        while right < width_i
            && pixel(pixels, width, height, right, y).is_some_and(|value| value == target)
        {
            right += 1;
        }
        right -= 1;
        if left > right {
            continue;
        }
        let mut above = false;
        let mut below = false;
        for column in left..=right {
            put_pixel(pixels, width, height, column, y, color);
            if !above
                && y > 0
                && pixel(pixels, width, height, column, y - 1).is_some_and(|value| value == target)
            {
                stack.push((column, y - 1));
                above = true;
            } else if above
                && y > 0
                && !pixel(pixels, width, height, column, y - 1).is_some_and(|value| value == target)
            {
                above = false;
            }
            if !below
                && y + 1 < height_i
                && pixel(pixels, width, height, column, y + 1).is_some_and(|value| value == target)
            {
                stack.push((column, y + 1));
                below = true;
            } else if below
                && y + 1 < height_i
                && !pixel(pixels, width, height, column, y + 1).is_some_and(|value| value == target)
            {
                below = false;
            }
        }
    }
    true
}

/// Put the old picture inside the new shape without stretching it.
pub fn fit_into(
    source: &[u8],
    source_width: u32,
    source_height: u32,
    destination: &mut [u8],
    width: u32,
    height: u32,
) {
    fill(destination, PAPER);
    if source_width == 0 || source_height == 0 || width == 0 || height == 0 {
        return;
    }
    let scale = (width as f32 / source_width as f32).min(height as f32 / source_height as f32);
    let scaled_width = ((source_width as f32 * scale).round() as u32).clamp(1, width);
    let scaled_height = ((source_height as f32 * scale).round() as u32).clamp(1, height);
    let left = (width - scaled_width) / 2;
    let top = (height - scaled_height) / 2;
    for y in 0..scaled_height {
        let source_y = ((u64::from(y) * u64::from(source_height)) / u64::from(scaled_height))
            .min(u64::from(source_height - 1));
        for x in 0..scaled_width {
            let source_x = ((u64::from(x) * u64::from(source_width)) / u64::from(scaled_width))
                .min(u64::from(source_width - 1));
            let source_index = ((source_y * u64::from(source_width) + source_x) * 4) as usize;
            let destination_index =
                ((u64::from(top + y) * u64::from(width) + u64::from(left + x)) * 4) as usize;
            destination[destination_index..destination_index + 4]
                .copy_from_slice(&source[source_index..source_index + 4]);
        }
    }
}

pub fn write_png(path: &Path, pixels: &[u8], width: u32, height: u32) -> Result<(), String> {
    let file = File::create(path).map_err(|error| format!("create {}: {error}", path.display()))?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|error| format!("PNG header {}: {error}", path.display()))?;
    writer
        .write_image_data(pixels)
        .map_err(|error| format!("PNG pixels {}: {error}", path.display()))
}

pub fn canvas_path(artifacts: &Path, name: Option<&str>) -> Result<PathBuf, String> {
    let relative = match name {
        Some(name) => {
            let relative = Path::new(name);
            if relative.as_os_str().is_empty()
                || relative.is_absolute()
                || relative.components().any(|component| {
                    matches!(
                        component,
                        Component::ParentDir | Component::RootDir | Component::Prefix(_)
                    )
                })
            {
                return Err("name must be relative to iw4l-artifacts/draw".into());
            }
            relative.to_path_buf()
        }
        None => PathBuf::from(format!(
            "draw-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_secs())
                .unwrap_or(0)
        )),
    };
    let mut path = artifacts.join("draw").join(relative);
    if path
        .extension()
        .is_some_and(|extension| !extension.to_string_lossy().eq_ignore_ascii_case("png"))
    {
        return Err("name must use a .png extension".into());
    }
    if path.extension().is_none() {
        path.set_extension("png");
    }
    Ok(path)
}
