//! Image file storage + GC for clipboard images (spec §7.4).

use std::path::{Path, PathBuf};

use image::{ImageBuffer, ImageFormat, Rgba};

/// Save an RGBA buffer as a PNG file inside `image_dir`.
///
/// The filename is derived from the first 16 hex characters of `hash_hex`. If a file with
/// the same name already exists (de-dup hit), the function returns the existing path without
/// rewriting.
pub fn save_image_png(
    image_dir: &Path,
    hash_hex: &str,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<PathBuf, String> {
    let prefix_len = hash_hex.len().min(16);
    let file_name = format!("{}.png", &hash_hex[..prefix_len]);
    let path = image_dir.join(file_name);

    if path.exists() {
        return Ok(path);
    }

    let buf: ImageBuffer<Rgba<u8>, _> = ImageBuffer::from_raw(width, height, rgba.to_vec())
        .ok_or_else(|| "RGBA buffer size does not match width*height*4".to_string())?;
    buf.save_with_format(&path, ImageFormat::Png)
        .map_err(|e| format!("save image: {e}"))?;
    Ok(path)
}

/// Delete all `.png` files in `image_dir` whose absolute string path is NOT in `referenced_paths`.
/// Uses rayon for parallel deletion. Returns the number of files deleted.
#[allow(dead_code)] // reserved: orphan image GC, invoked from future retention hook
pub fn gc_orphan_images(
    image_dir: &Path,
    referenced_paths: &std::collections::HashSet<String>,
) -> Result<u64, String> {
    use rayon::prelude::*;

    let files: Vec<PathBuf> = std::fs::read_dir(image_dir)
        .map_err(|e| format!("read dir: {e}"))?
        .filter_map(|r| r.ok().map(|d| d.path()))
        .filter(|p| p.extension().map(|e| e == "png").unwrap_or(false))
        .collect();

    let deleted: u64 = files
        .par_iter()
        .filter(|p| {
            let s = p.to_string_lossy().to_string();
            !referenced_paths.contains(&s)
        })
        .map(|p| {
            if std::fs::remove_file(p).is_ok() {
                1u64
            } else {
                0
            }
        })
        .sum();

    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn save_image_writes_png() {
        let dir = TempDir::new().unwrap();
        // 2x2 RGBA (white)
        let rgba = vec![255u8; 4 * 2 * 2];
        let path = save_image_png(dir.path(), "deadbeef1234567890abcdef", 2, 2, &rgba).unwrap();
        assert!(path.exists());
        assert!(path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with(".png"));
    }

    #[test]
    fn save_image_is_idempotent_on_hash_hit() {
        let dir = TempDir::new().unwrap();
        let rgba = vec![255u8; 4];
        let p1 = save_image_png(dir.path(), "samehash0123456789abcd", 1, 1, &rgba).unwrap();
        let p2 = save_image_png(dir.path(), "samehash0123456789abcd", 1, 1, &rgba).unwrap();
        assert_eq!(p1, p2);
    }

    #[test]
    fn gc_removes_orphans() {
        let dir = TempDir::new().unwrap();
        let rgba = vec![255u8; 4];
        let kept = save_image_png(dir.path(), "referenced_hashxxxx", 1, 1, &rgba).unwrap();
        let _ = save_image_png(dir.path(), "orphan_hashabcdyyyy", 1, 1, &rgba).unwrap();

        let mut referenced = std::collections::HashSet::new();
        referenced.insert(kept.to_string_lossy().to_string());

        let deleted = gc_orphan_images(dir.path(), &referenced).unwrap();
        assert_eq!(deleted, 1);
        assert!(kept.exists());
    }
}
