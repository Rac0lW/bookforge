use std::{
    fs,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn config_cli() {
    let root = std::env::temp_dir().join(format!(
        "bookforge-cli-config-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let source = root.join("pages");
    let destination = root.join("epubs");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir(&destination).unwrap();
    image::RgbImage::new(1, 1)
        .save(source.join("1.png"))
        .unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_bookforge"))
            .env("HOME", &root)
            .args(args)
            .output()
            .unwrap()
    };
    let missing = run(&["open"]);
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("bookforge init"));
    assert!(run(&["init"]).status.success());
    let config = root.join(".config/bookforge/config.toml");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let bin = root.join("bin");
        fs::create_dir(&bin).unwrap();
        let opener = bin.join(if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        });
        fs::write(
            &opener,
            "#!/bin/sh\nprintf '%s' \"$1\" > \"$BOOKFORGE_TEST_OPEN_LOG\"\n",
        )
        .unwrap();
        fs::set_permissions(&opener, fs::Permissions::from_mode(0o755)).unwrap();
        let log = root.join("opened");
        let result = Command::new(env!("CARGO_BIN_EXE_bookforge"))
            .env("HOME", &root)
            .env(
                "PATH",
                std::env::join_paths(
                    std::iter::once(bin.clone())
                        .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
                )
                .unwrap(),
            )
            .env("BOOKFORGE_TEST_OPEN_LOG", &log)
            .arg("open")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(fs::read_to_string(log).unwrap(), config.to_str().unwrap());
    }
    let original = fs::read(&config).unwrap();
    let result = run(&["init"]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("未覆盖"));
    assert_eq!(fs::read(&config).unwrap(), original);
    assert!(
        run(&["config", "set", "output_dir", destination.to_str().unwrap()])
            .status
            .success()
    );
    assert!(run(&["config", "set", "books", "true"]).status.success());
    let result = run(&[source.to_str().unwrap(), "--dry-run", "--no-books"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains(&format!(
        "输出：{}",
        destination.join("pages.epub").display()
    )));
    let result = run(&[
        source.to_str().unwrap(),
        "--dry-run",
        "--no-books",
        "-o",
        "other",
    ]);
    assert!(result.status.success());
    assert!(String::from_utf8_lossy(&result.stdout).contains("输出：other.epub"));
    assert!(!destination.join("pages.epub").exists());
    fs::remove_dir_all(root).unwrap();
}
