//! Les couleurs des pistes et des canaux.

use slint::Color;

pub const COLORS: [u32; 10] = [0xFF7A5C, 0xF5B547, 0xA6D96A, 0x4FD1A5, 0x3CC8D8, 0x5AA9FF, 0x8C8CFF, 0xC77DFF, 0xFF6FA8, 0xD9B38C];

pub fn color(i: usize) -> Color {
    let c = COLORS[i % COLORS.len()];
    Color::from_rgb_u8((c >> 16) as u8, (c >> 8) as u8, c as u8)
}
