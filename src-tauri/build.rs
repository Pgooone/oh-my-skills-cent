fn main() {
    // tauri_build::build() requires the tauri dependency tree; skip it when the
    // desktop shell feature is off (e.g. web-only builds).
    if std::env::var_os("CARGO_FEATURE_TAURI_SHELL").is_some() {
        tauri_build::build()
    }
}
