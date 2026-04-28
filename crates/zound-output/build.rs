// macOS: транзитивная зависимость screencapturekit-rs (через zound-platform)
// линкует Swift-биндинги, но не добавляет rpath к Swift runtime. На GitHub
// Actions macos-latest libswift_Concurrency.dylib не находится в dyld
// cache, и любой тестовый бинарь, в который попадает screencapturekit,
// падает SIGABRT ещё до main(). /usr/lib/swift — стандартное место
// Swift-рантайма на macOS 12+.
//
// `cargo:rustc-link-arg-tests` независим от RUSTFLAGS env var (которая
// в CI содержит `-D warnings` и переопределила бы любой
// .cargo/config.toml rustflags), поэтому фикс делается в build.rs того
// крейта, чей тестовый бинарь требует Swift-рантайм.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg-tests=-Wl,-rpath,/usr/lib/swift");
    }
}
