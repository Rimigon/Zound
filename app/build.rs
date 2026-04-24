//! Build-script. Tauri требует `icons/icon.ico` на Windows для
//! встраивания в Windows Resource. Если файла нет — генерируем 16x16
//! placeholder с фирменным синим цветом Zound. Настоящая иконка
//! появится, когда будет готова графика.

use std::fs;
use std::path::Path;

fn main() {
    ensure_placeholder_icons();
    tauri_build::build();
}

fn ensure_placeholder_icons() {
    let ico_path = Path::new("icons/icon.ico");
    if !ico_path.exists() {
        fs::create_dir_all("icons").expect("create icons/");
        fs::write(ico_path, minimal_ico()).expect("write placeholder icon.ico");
    }
}

/// Минимально валидный ICO-файл: 16x16, 32 bpp, заполненный цветом Zound.
fn minimal_ico() -> Vec<u8> {
    const W: i32 = 16;
    const H: i32 = 16;
    const BPP: u16 = 32;
    const PIXEL_BYTES: u32 = (W as u32) * (H as u32) * 4;
    const MASK_BYTES: u32 = (W as u32) * (H as u32) / 8;
    const HEADER_BYTES: u32 = 40;
    const IMAGE_SIZE: u32 = HEADER_BYTES + PIXEL_BYTES + MASK_BYTES;

    let mut v = Vec::with_capacity(6 + 16 + IMAGE_SIZE as usize);

    // ICONDIR: reserved=0, type=1 (icon), count=1.
    v.extend_from_slice(&[0x00, 0x00, 0x01, 0x00, 0x01, 0x00]);

    // ICONDIRENTRY: width, height, colors, reserved, planes, bpp, size, offset.
    v.push(W as u8);
    v.push(H as u8);
    v.push(0); // colors in palette
    v.push(0); // reserved
    v.extend_from_slice(&1u16.to_le_bytes()); // planes
    v.extend_from_slice(&BPP.to_le_bytes()); // bpp
    v.extend_from_slice(&IMAGE_SIZE.to_le_bytes());
    v.extend_from_slice(&22u32.to_le_bytes()); // offset to image data

    // BITMAPINFOHEADER (40 bytes). Height = 2*H для XOR+AND масок.
    v.extend_from_slice(&HEADER_BYTES.to_le_bytes());
    v.extend_from_slice(&W.to_le_bytes());
    v.extend_from_slice(&(H * 2).to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&BPP.to_le_bytes());
    v.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    v.extend_from_slice(&(PIXEL_BYTES + MASK_BYTES).to_le_bytes());
    v.extend_from_slice(&0u32.to_le_bytes());
    v.extend_from_slice(&0u32.to_le_bytes());
    v.extend_from_slice(&0u32.to_le_bytes());
    v.extend_from_slice(&0u32.to_le_bytes());

    // XOR mask: BGRA, цвет #6AA8FF (синий Zound).
    for _ in 0..(W * H) {
        v.extend_from_slice(&[0xFF, 0xA8, 0x6A, 0xFF]);
    }
    // AND mask: все нули — непрозрачно.
    v.extend(std::iter::repeat(0u8).take(MASK_BYTES as usize));
    v
}
