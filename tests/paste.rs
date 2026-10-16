#![cfg(target_os = "macos")]

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

fn invoke(root: &Path, clipboard: &str, dry: bool) -> Output {
    let tool = root.join("pbpaste");
    fs::write(&tool, format!("#!/bin/sh\nprintf '%s' '{clipboard}'\n")).unwrap();
    fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_bookforge"));
    command
        .arg("--no-books")
        .arg("-p")
        .arg("-o")
        .arg(root.join("pasted.epub"))
        .env("PATH", root);
    if dry {
        command.arg("--dry-run");
    }
    command.output().unwrap()
}

#[test]
fn paste_reads_input_from_clipboard() {
    let root = std::env::temp_dir().join(format!(
        "bookforge-paste-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let images = root.join("images");
    fs::create_dir_all(&images).unwrap();
    image::RgbaImage::from_pixel(2, 3, image::Rgba([1, 2, 3, 255]))
        .save(images.join("1.png"))
        .unwrap();
    let clipboard = format!("  {}\n", images.display());

    let preview = invoke(&root, &clipboard, true);
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    let stdout = String::from_utf8_lossy(&preview.stdout);
    assert!(stdout.contains("剪贴板输入："));
    assert!(stdout.contains("1.png [封面]"));
    assert!(!root.join("pasted.epub").exists());

    let result = invoke(&root, &clipboard, false);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(root.join("pasted.epub").exists());

    let empty = invoke(&root, " ", true);
    assert!(!empty.status.success());
    assert!(String::from_utf8_lossy(&empty.stderr).contains("剪贴板为空"));
    fs::remove_dir_all(root).unwrap();
}
