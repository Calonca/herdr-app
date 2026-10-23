//! Embeds the default mods (defaults/mods/**), the program icons
//! (defaults/icons/*) and the frame's mods (defaults/frame/**) so Herdr can
//! install them.

use std::path::Path;

fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            walk(&p, root, out);
        } else {
            println!("cargo:rerun-if-changed={}", p.display());
            let rel = p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            out.push(format!("    ({rel:?}, include_bytes!({:?})),", p.canonicalize().unwrap()));
        }
    }
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("defaults/mods");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut lines = Vec::new();
    walk(&root, &root, &mut lines);
    let code = format!("pub const DEFAULT_MODS: &[(&str, &[u8])] = &[\n{}\n];\n", lines.join("\n"));
    std::fs::write(Path::new(&std::env::var("OUT_DIR").unwrap()).join("default_mods.rs"), code).unwrap();

    let icons = Path::new(env!("CARGO_MANIFEST_DIR")).join("defaults/icons");
    println!("cargo:rerun-if-changed={}", icons.display());
    let mut lines = Vec::new();
    walk(&icons, &icons, &mut lines);
    let code = format!("pub const DEFAULT_ICONS: &[(&str, &[u8])] = &[\n{}\n];\n", lines.join("\n"));
    std::fs::write(Path::new(&std::env::var("OUT_DIR").unwrap()).join("default_icons.rs"), code).unwrap();

    let frame = Path::new(env!("CARGO_MANIFEST_DIR")).join("defaults/frame");
    println!("cargo:rerun-if-changed={}", frame.display());
    let mut lines = Vec::new();
    walk(&frame, &frame, &mut lines);
    let code = format!("pub const FRAME_MODS: &[(&str, &[u8])] = &[\n{}\n];\n", lines.join("\n"));
    std::fs::write(Path::new(&std::env::var("OUT_DIR").unwrap()).join("frame_mods.rs"), code).unwrap();

    // Windows takes the window, taskbar and Start menu icon from the program file.
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=assets/icons/herdr-app.ico");
        winresource::WindowsResource::new().set_icon("assets/icons/herdr-app.ico").compile().expect("cannot embed the icon");
    }
}
