fn main() {
    // The licence texts are generated at build time (`bun run licenses`, run by beforeBuildCommand) and
    // not committed. A plain `cargo build` or `cargo test` before that still needs the bundled resource
    // to exist, so it gets a stub that says how to make the real one. Release builds always regenerate it.
    let licenses = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources").join("THIRD_PARTY_LICENSES.txt");
    if !licenses.exists() {
        let _ = std::fs::create_dir_all(licenses.parent().unwrap());
        let _ = std::fs::write(
            &licenses,
            "NOT GENERATED: this is a development build. Run `bun run licenses` to write the licence texts.\n",
        );
    }
    tauri_build::build()
}
