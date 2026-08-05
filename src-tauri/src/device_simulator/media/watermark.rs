use chrono::NaiveDateTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatermarkLayout {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    pub scale: usize,
}

pub fn format_time_watermark(value: NaiveDateTime) -> String {
    value.format("%Y/%m/%d %H:%M:%S").to_string()
}

/// Burns the fixed ASCII timestamp into an NV12 frame. The renderer uses a
/// small bundled bitmap alphabet so Worker behavior never depends on installed
/// fonts, locale fallback, GDI state, or a desktop session.
pub fn render_time_watermark_nv12(
    frame: &mut [u8],
    width: usize,
    height: usize,
    text: &str,
) -> Result<WatermarkLayout, String> {
    if width < 64 || height < 32 || width % 2 != 0 || height % 2 != 0 {
        return Err("time watermark requires an even NV12 frame of at least 64x32".into());
    }
    let required = width
        .checked_mul(height)
        .and_then(|luma| luma.checked_add(luma / 2))
        .ok_or_else(|| "NV12 frame dimensions overflow".to_string())?;
    if frame.len() < required {
        return Err("NV12 frame is shorter than its declared dimensions".into());
    }
    if text.is_empty() || !text.bytes().all(|value| glyph(value).is_some()) {
        return Err("time watermark contains an unsupported character".into());
    }

    let mut scale = (height / 180).clamp(1, 6);
    let text_len = text.len();
    loop {
        let advance = 6 * scale;
        let text_width = text_len
            .checked_mul(advance)
            .and_then(|value| value.checked_sub(scale))
            .ok_or_else(|| "time watermark width overflow".to_string())?;
        let padding = 2 * scale;
        let margin = (4 * scale).max(4);
        if text_width + padding * 2 + margin <= width || scale == 1 {
            break;
        }
        scale -= 1;
    }

    let advance = 6 * scale;
    let text_width = text_len * advance - scale;
    let text_height = 7 * scale;
    let padding = 2 * scale;
    // Keep the background anchored to the video top-right corner while
    // giving the timestamp glyphs an even black inset on all four sides.
    let box_width = text_width + padding * 2;
    let box_height = text_height + padding * 2;
    if box_width > width || box_height > height {
        return Err("time watermark does not fit inside the NV12 frame".into());
    }
    let x = width - box_width;
    let y = 0;
    let layout = WatermarkLayout {
        x,
        y,
        width: box_width,
        height: box_height,
        scale,
    };

    fill_nv12_rect(frame, width, height, layout, 32, 128);
    let text_x = x + padding;
    let text_y = y + padding;
    for (character_index, character) in text.bytes().enumerate() {
        let bitmap = glyph(character).expect("validated watermark glyph");
        for (row, bits) in bitmap.iter().copied().enumerate() {
            for column in 0..5 {
                if bits & (1 << (4 - column)) == 0 {
                    continue;
                }
                let pixel_x = text_x + character_index * advance + column * scale;
                let pixel_y = text_y + row * scale;
                for dy in 0..scale {
                    let start = (pixel_y + dy) * width + pixel_x;
                    frame[start..start + scale].fill(235);
                }
            }
        }
    }
    Ok(layout)
}

fn fill_nv12_rect(
    frame: &mut [u8],
    width: usize,
    height: usize,
    layout: WatermarkLayout,
    luma: u8,
    chroma: u8,
) {
    for row in layout.y..layout.y + layout.height {
        let start = row * width + layout.x;
        frame[start..start + layout.width].fill(luma);
    }

    let luma_size = width * height;
    let chroma_x = layout.x & !1;
    let chroma_end = (layout.x + layout.width + 1).min(width) & !1;
    let chroma_y = layout.y / 2;
    let chroma_end_y = (layout.y + layout.height + 1).min(height) / 2;
    for row in chroma_y..chroma_end_y {
        let start = luma_size + row * width + chroma_x;
        frame[start..luma_size + row * width + chroma_end].fill(chroma);
    }
}

fn glyph(value: u8) -> Option<&'static [u8; 7]> {
    const DIGITS: [[u8; 7]; 10] = [
        [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        [
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        [
            0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110,
        ],
        [
            0b01110, 0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110,
        ],
    ];
    const SLASH: [u8; 7] = [
        0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000,
    ];
    const COLON: [u8; 7] = [0, 0b00100, 0b00100, 0, 0b00100, 0b00100, 0];
    const SPACE: [u8; 7] = [0; 7];
    match value {
        b'0'..=b'9' => Some(&DIGITS[(value - b'0') as usize]),
        b'/' => Some(&SLASH),
        b':' => Some(&COLON),
        b' ' => Some(&SPACE),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn formats_the_approved_local_time_shape() {
        let value = NaiveDate::from_ymd_opt(2026, 7, 27)
            .unwrap()
            .and_hms_opt(15, 0, 0)
            .unwrap();
        assert_eq!(format_time_watermark(value), "2026/07/27 15:00:00");
    }

    #[test]
    fn renders_against_1080p_and_360p_top_right_edges() {
        for (width, height) in [(1_920usize, 1_080usize), (640, 360)] {
            let mut frame = vec![128; width * height * 3 / 2];
            let layout =
                render_time_watermark_nv12(&mut frame, width, height, "2026/07/27 15:00:00")
                    .unwrap();
            assert_eq!(layout.x + layout.width, width);
            assert!(layout.y + layout.height < height);
            assert!(layout.x > width / 2);
            assert!(frame[..width * height].iter().any(|value| *value == 235));
            assert!(frame[..width * height].iter().any(|value| *value == 32));

            let text_x = layout.x + 2 * layout.scale;
            let text_y = layout.y + 2 * layout.scale;
            assert_eq!(layout.y, 0);
            assert_eq!(
                frame[layout.y * width + text_x + layout.scale],
                32,
                "the timestamp should retain black padding above the glyphs",
            );
            assert_eq!(
                frame[(text_y + layout.scale) * width + width - 1],
                32,
                "the timestamp should retain black padding after the final digit",
            );
            assert_eq!(
                frame[text_y * width + text_x + layout.scale],
                235,
                "the first timestamp digit should begin after the left padding",
            );
            assert_eq!(
                frame[(text_y + layout.scale) * width + width - 2 * layout.scale - 1],
                235,
                "the final timestamp digit should end before the right padding",
            );
        }
    }

    #[test]
    fn rejects_short_or_unsupported_frames() {
        assert!(render_time_watermark_nv12(
            &mut vec![0; 640 * 360],
            640,
            360,
            "2026/07/27 15:00:00"
        )
        .is_err());
        assert!(render_time_watermark_nv12(
            &mut vec![0; 640 * 360 * 3 / 2],
            640,
            360,
            "2026-07-27"
        )
        .is_err());
    }
}
