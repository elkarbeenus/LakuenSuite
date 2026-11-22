fn main() {
    // Version für die Anzeige: LAKUEN_DESCRIBE aus der CI, sonst git describe.
    let describe = std::env::var("LAKUEN_DESCRIBE").ok().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| {
        std::process::Command::new("git")
            .args(["describe", "--tags", "--long", "--always", "--match", "v*"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default()
    });
    println!("cargo:rustc-env=LAKUEN_DESCRIBE={}", describe);
    println!("cargo:rerun-if-env-changed=LAKUEN_DESCRIBE");
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/refs/tags");
    tauri_build::build()
}
