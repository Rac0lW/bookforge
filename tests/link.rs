#![cfg(unix)]

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn link_download_preview_and_missing_tool() {
    let root = std::env::temp_dir().join(format!(
        "bookforge-link-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let png = root.join("fixture.png");
    image::RgbaImage::from_pixel(2, 3, image::Rgba([1, 2, 3, 255]))
        .save(&png)
        .unwrap();
    let tool = root.join("gallery-dl");
    fs::write(
        &tool,
        format!(
            "#!/bin/sh\nif [ \"$1\" = --version ]; then echo fake; exit 0; fi\n[ \"$1\" = -D ] && [ \"$3\" = -- ] && [ \"$4\" = 'https://example.org/album?x=1' ] || exit 3\n/bin/cp '{}' \"$2/001.png\"\n",
            png.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();
    let output = root.join("album.epub");
    let invoke = |dry| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_bookforge"));
        command
            .arg("https://example.org/album?x=1")
            .arg("-o")
            .arg(&output)
            .env("PATH", &root);
        if dry {
            command.arg("--dry-run");
        }
        command.output().unwrap()
    };
    let preview = invoke(true);
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    assert!(String::from_utf8_lossy(&preview.stdout).contains("001.png [封面]"));
    assert!(!output.exists());
    let result = invoke(false);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(output.exists());
    fs::remove_file(&tool).unwrap();
    let missing = invoke(false);
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("未找到 gallery-dl"));
    fs::remove_dir_all(root).unwrap();
}
