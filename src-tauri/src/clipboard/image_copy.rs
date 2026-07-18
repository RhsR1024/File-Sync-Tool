//! Image-file to Windows image-data clipboard support.
//!
//! The Windows path deliberately exposes the same broad set of formats as ImageGlass:
//! registered PNG, CF_DIBV5 (through arboard), and a standard CF_BITMAP fallback.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use arboard::{Clipboard, ImageData};
use image::{DynamicImage, GenericImageView, ImageDecoder, ImageReader};
use serde::{Deserialize, Serialize};

const MAX_DECODED_PIXELS: u64 = 100_000_000;

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct ImageCrop {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImageCopyResult {
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub cropped: bool,
}

pub fn copy_image_file(path: &Path, crop: Option<ImageCrop>) -> Result<ImageCopyResult, String> {
    let canonical_path = validate_image_path(path)?;
    let mut image = decode_oriented_image(&canonical_path)?;
    let cropped = crop.is_some();

    if let Some(selection) = crop {
        validate_crop(image.dimensions(), selection)?;
        image = image.crop_imm(selection.x, selection.y, selection.width, selection.height);
    }

    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    let bytes = rgba.into_raw();

    let mut clipboard = Clipboard::new().map_err(|error| format!("clipboard init: {error}"))?;
    clipboard
        .set_image(ImageData {
            width: width as usize,
            height: height as usize,
            bytes: Cow::Borrowed(&bytes),
        })
        .map_err(|error| format!("copy image: {error}"))?;

    #[cfg(target_os = "windows")]
    add_standard_bitmap(width, height, &bytes)?;

    Ok(ImageCopyResult {
        path: canonical_path.to_string_lossy().to_string(),
        width,
        height,
        cropped,
    })
}

fn validate_image_path(path: &Path) -> Result<PathBuf, String> {
    if !path.is_file() {
        return Err("image file does not exist".to_string());
    }

    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(extension.as_str(), "png" | "jpg" | "jpeg") {
        return Err("only PNG and JPEG images are supported".to_string());
    }

    path.canonicalize()
        .map_err(|error| format!("resolve image path: {error}"))
}

fn decode_oriented_image(path: &Path) -> Result<DynamicImage, String> {
    let reader = ImageReader::open(path)
        .map_err(|error| format!("open image: {error}"))?
        .with_guessed_format()
        .map_err(|error| format!("detect image format: {error}"))?;
    let mut decoder = reader
        .into_decoder()
        .map_err(|error| format!("decode image header: {error}"))?;
    let (width, height) = decoder.dimensions();
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or_else(|| "image dimensions are too large".to_string())?;
    if pixels == 0 || pixels > MAX_DECODED_PIXELS {
        return Err(format!(
            "image contains {pixels} pixels; maximum is {MAX_DECODED_PIXELS}"
        ));
    }

    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut image =
        DynamicImage::from_decoder(decoder).map_err(|error| format!("decode image: {error}"))?;
    image.apply_orientation(orientation);
    Ok(image)
}

fn validate_crop(dimensions: (u32, u32), crop: ImageCrop) -> Result<(), String> {
    let (image_width, image_height) = dimensions;
    let right = crop
        .x
        .checked_add(crop.width)
        .ok_or_else(|| "crop rectangle is invalid".to_string())?;
    let bottom = crop
        .y
        .checked_add(crop.height)
        .ok_or_else(|| "crop rectangle is invalid".to_string())?;
    if crop.width == 0 || crop.height == 0 || right > image_width || bottom > image_height {
        return Err("crop rectangle is outside the image".to_string());
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn add_standard_bitmap(width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    use clipboard_win::options::NoClear;

    let bitmap = build_bmp(width, height, rgba)?;
    let _clipboard = clipboard_win::Clipboard::new_attempts(10)
        .map_err(|error| format!("open clipboard for bitmap: {error}"))?;
    clipboard_win::raw::set_bitmap_with(&bitmap, NoClear)
        .map_err(|error| format!("set standard bitmap: {error}"))
}

#[cfg(target_os = "windows")]
fn build_bmp(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    const FILE_HEADER_SIZE: usize = 14;
    const INFO_HEADER_SIZE: usize = 40;
    const PIXEL_OFFSET: usize = FILE_HEADER_SIZE + INFO_HEADER_SIZE;

    let pixel_len = usize::try_from(width)
        .ok()
        .and_then(|value| value.checked_mul(height as usize))
        .and_then(|value| value.checked_mul(4))
        .ok_or_else(|| "image dimensions are too large".to_string())?;
    if rgba.len() != pixel_len {
        return Err("RGBA buffer size does not match image dimensions".to_string());
    }
    let file_size = PIXEL_OFFSET
        .checked_add(pixel_len)
        .ok_or_else(|| "bitmap is too large".to_string())?;
    let file_size_u32 = u32::try_from(file_size).map_err(|_| "bitmap is too large".to_string())?;
    let pixel_len_u32 = u32::try_from(pixel_len).map_err(|_| "bitmap is too large".to_string())?;
    let width_i32 = i32::try_from(width).map_err(|_| "image width is too large".to_string())?;
    let height_i32 = i32::try_from(height).map_err(|_| "image height is too large".to_string())?;

    let mut output = Vec::with_capacity(file_size);
    output.extend_from_slice(b"BM");
    output.extend_from_slice(&file_size_u32.to_le_bytes());
    output.extend_from_slice(&[0; 4]);
    output.extend_from_slice(&(PIXEL_OFFSET as u32).to_le_bytes());

    output.extend_from_slice(&(INFO_HEADER_SIZE as u32).to_le_bytes());
    output.extend_from_slice(&width_i32.to_le_bytes());
    output.extend_from_slice(&height_i32.to_le_bytes());
    output.extend_from_slice(&1u16.to_le_bytes());
    output.extend_from_slice(&32u16.to_le_bytes());
    output.extend_from_slice(&0u32.to_le_bytes());
    output.extend_from_slice(&pixel_len_u32.to_le_bytes());
    output.extend_from_slice(&0i32.to_le_bytes());
    output.extend_from_slice(&0i32.to_le_bytes());
    output.extend_from_slice(&0u32.to_le_bytes());
    output.extend_from_slice(&0u32.to_le_bytes());

    let row_bytes = width as usize * 4;
    for row in rgba.chunks_exact(row_bytes).rev() {
        for pixel in row.chunks_exact(4) {
            output.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 0xff]);
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_validation_rejects_out_of_bounds_selection() {
        let error = validate_crop(
            (100, 80),
            ImageCrop {
                x: 90,
                y: 10,
                width: 20,
                height: 20,
            },
        )
        .unwrap_err();
        assert!(error.contains("outside"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn bmp_builder_writes_bottom_up_bgra_pixels() {
        let rgba = [255, 0, 0, 255, 0, 255, 0, 128];
        let bmp = build_bmp(1, 2, &rgba).unwrap();
        assert_eq!(&bmp[0..2], b"BM");
        assert_eq!(&bmp[54..58], &[0, 255, 0, 255]);
        assert_eq!(&bmp[58..62], &[0, 0, 255, 255]);
    }
}
