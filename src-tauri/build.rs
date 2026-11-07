fn main() {
    // The licence texts are generated at build time (`bun run licenses`, run by beforeBuildCommand) and
    // not committed. A plain `cargo build` or `cargo test` before that still needs the bundled resource
    // to exist, so it gets a stub that says how to make the real one. Release builds always regenerate it.
    // CARGO_MANIFEST_DIR read at run time, not baked in with env!(): worktrees that share one
    // CARGO_TARGET_DIR reuse this compiled script, and a baked path points at whichever worktree
    // built it first (the stub then lands there and this build fails on the missing resource).
    let dir = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR for build scripts");
    let licenses = std::path::Path::new(&dir).join("resources").join("THIRD_PARTY_LICENSES.txt");
    if !licenses.exists() {
        let _ = std::fs::create_dir_all(licenses.parent().unwrap());
        let _ = std::fs::write(
            &licenses,
            "NOT GENERATED: this is a development build. Run `bun run licenses` to write the licence texts.\n",
        );
    }
    tauri_build::build()
}
