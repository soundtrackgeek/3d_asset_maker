fn main() {
    // tauri.conf.json bundles ../target/release/t2.exe; tauri-build checks that it exists
    // even for plain `cargo build`. Create a placeholder on a clean tree. The release script
    // always builds the real t2.exe first (`cargo build --release -p trellis-studio-cli`).
    let t2 = std::path::Path::new("../target/release/t2.exe");
    if !t2.exists() {
        if let Some(dir) = t2.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(t2, b"placeholder");
    }
    tauri_build::build()
}
