// macOS: транзитивная зависимость screencapturekit-rs (через zound-platform)
// линкует Swift-биндинги, но не добавляет rpath к Swift runtime. На GitHub
// Actions macos-latest libswift_Concurrency.dylib не находится в dyld
// cache, и любой линкуемый артефакт, в который попадает screencapturekit,
// падает SIGABRT ещё до main(). /usr/lib/swift — стандартное место
// Swift-рантайма на macOS 12+.
//
// `cargo:rustc-link-arg-tests` (LinkArgTarget::Test) применяется только
// к target.kind() == TargetKind::Test, т.е. к интеграционным тестам в
// `tests/*.rs`. Юнит-тесты `--lib` имеют target.kind() == Lib и mode = Test
// — к ним применяется только LinkArgTarget::All (`rustc-link-arg`).
// Поэтому используем безсуффиксную директиву: cargo сам отфильтрует
// нелинкуемые единицы (plain rlib).
//
// Директивы build-script независимы от RUSTFLAGS env var (которая в CI
// содержит `-D warnings` и переопределила бы любой
// `.cargo/config.toml`-rustflags), поэтому фикс живёт здесь, в build.rs
// того крейта, чьи тестовые бинари требуют Swift-рантайм.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }
}
