//! Build-script. Делает три вещи:
//!
//! 1. Генерирует placeholder `icons/icon.ico` если его нет.
//! 2. Парсит `locales/en.ftl` и `locales/ru.ftl`, генерирует
//!    `OUT_DIR/i18n_keys.rs` с `KEYS` для `i18n.rs`. Так список ключей
//!    больше не дублируется руками — паритет ru↔en проверяется на
//!    этапе сборки и роняет компиляцию при расхождении.
//! 3. Вызывает `tauri_build::build()` для встраивания ресурсов.

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::Path;

fn main() {
    ensure_placeholder_icons();
    generate_i18n_keys();
    emit_macos_swift_rpath();
    tauri_build::build();
}

/// macOS: транзитивная зависимость screencapturekit-rs (через
/// zound-platform) линкует Swift-биндинги, но не добавляет rpath к
/// Swift runtime. На GitHub Actions macos-latest libswift_Concurrency.dylib
/// не находится в dyld cache → SIGABRT ещё до main(). /usr/lib/swift —
/// стандартное место Swift-рантайма на macOS 12+. `cargo:rustc-link-arg`
/// независим от RUSTFLAGS env var (которая в CI содержит `-D warnings`
/// и переопределила бы `.cargo/config.toml`), поэтому фикс делается в
/// build.rs.
fn emit_macos_swift_rpath() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,/usr/lib/swift");
        println!("cargo:rustc-link-arg-tests=-Wl,-rpath,/usr/lib/swift");
    }
}

/// Парсит .ftl-файлы и пишет в `OUT_DIR/i18n_keys.rs`:
///
/// ```ignore
/// pub const KEYS: &[&str] = &["app-title", "app-subtitle", ...];
/// ```
///
/// Если en и ru расходятся по набору ключей — `panic!` на стадии build,
/// то есть `cargo check` сразу красный и нельзя случайно зарелизить
/// приложение с пустыми переводами. Парсер тривиальный (top-level
/// `<id> = ...`), но он корректен для нашего использования fluent —
/// атрибутов и selector-ов в `.ftl` мы не используем.
fn generate_i18n_keys() {
    let en_path = Path::new("../locales/en.ftl");
    let ru_path = Path::new("../locales/ru.ftl");
    println!("cargo:rerun-if-changed=../locales/en.ftl");
    println!("cargo:rerun-if-changed=../locales/ru.ftl");

    let en_src = fs::read_to_string(en_path).expect("read locales/en.ftl");
    let ru_src = fs::read_to_string(ru_path).expect("read locales/ru.ftl");

    let en_keys = parse_ftl_message_ids(&en_src);
    let ru_keys = parse_ftl_message_ids(&ru_src);

    let missing_in_ru: Vec<&String> = en_keys.difference(&ru_keys).collect();
    let missing_in_en: Vec<&String> = ru_keys.difference(&en_keys).collect();
    if !missing_in_ru.is_empty() || !missing_in_en.is_empty() {
        panic!(
            "i18n parity broken: missing in ru.ftl = {missing_in_ru:?}, missing in en.ftl = {missing_in_en:?}"
        );
    }

    let mut all: Vec<String> = en_keys.into_iter().collect();
    all.sort();
    let mut out = String::from(
        "// Auto-generated from locales/*.ftl by app/build.rs.\n\
         // Do not edit by hand.\n\
         pub const KEYS: &[&str] = &[\n",
    );
    for k in &all {
        out.push_str("    \"");
        out.push_str(k);
        out.push_str("\",\n");
    }
    out.push_str("];\n");

    let out_dir = env::var_os("OUT_DIR").expect("OUT_DIR set by cargo");
    let dest = Path::new(&out_dir).join("i18n_keys.rs");
    fs::write(&dest, out).expect("write i18n_keys.rs");
}

/// Возвращает множество top-level message-id из FTL-файла. Top-level —
/// это идентификатор слева от `=`, без точки (атрибуты — `key.attr =`)
/// и не начинающийся с `-` (термы). Multiline-значения и комментарии
/// (`#`) пропускаем естественным образом — мы матчим только строки,
/// начинающиеся с буквы и содержащие `=`.
fn parse_ftl_message_ids(src: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in src.lines() {
        let trimmed = line.trim_start();
        // Только строки, не начинающиеся с пробела/комментария/терма —
        // это и есть top-level message id (continuation-строки идут с
        // отступом по правилам Fluent).
        if line != trimmed {
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('-') {
            continue;
        }
        if let Some(eq_idx) = trimmed.find('=') {
            let id = trimmed[..eq_idx].trim();
            // Атрибуты (`key.attr = ...`) пропускаем — нам нужен только
            // основной message-id.
            if id.contains('.') {
                continue;
            }
            // Валидный fluent id: [a-zA-Z][a-zA-Z0-9_-]*.
            if id
                .chars()
                .next()
                .map(|c| c.is_ascii_alphabetic())
                .unwrap_or(false)
                && id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                out.insert(id.to_string());
            }
        }
    }
    out
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
