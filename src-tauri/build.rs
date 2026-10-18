fn main() {
    // Bundled adb lives here once `pnpm fetch-adb` has run; the directory must exist for
    // `bundle.resources` even when it has not, and the app then falls back to adb in PATH.
    std::fs::create_dir_all("resources/platform-tools").expect("create resources/platform-tools");
    tauri_build::build()
}
