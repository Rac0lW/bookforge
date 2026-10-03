use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

fn invoke(source: &Path, output: &Path, flag: &str, answer: &str, dry: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_bookforge"));
    command
        .arg(source)
        .args(["--no-books", flag, "-o"])
        .arg(output)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if dry {
        command.arg("--dry-run");
    }
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(answer.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn deletion_requires_confirmation_and_successful_output() {
    let root = std::env::temp_dir().join(format!(
        "bookforge-delete-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let png = root.join("page.png");
    image::RgbaImage::from_pixel(2, 3, image::Rgba([1, 2, 3, 255]))
        .save(&png)
        .unwrap();
    let bytes = fs::read(&png).unwrap();
    for archive in [false, true] {
        for (index, (answer, dry, fail, deleted)) in [
            ("y\n", false, false, true),
            ("Y\n", false, false, true),
            ("n\n", false, false, false),
            ("\n", false, false, false),
            ("", false, false, false),
            ("unexpected\n", false, false, false),
            ("y\n", true, false, false),
            ("y\n", false, true, false),
        ]
        .into_iter()
        .enumerate()
        {
            let source = root.join(format!(
                "source-{archive}-{index}{}",
                if archive { ".zip" } else { "" }
            ));
            if archive {
                let mut zip = ZipWriter::new(fs::File::create(&source).unwrap());
                zip.start_file("page.png", SimpleFileOptions::default())
                    .unwrap();
                zip.write_all(&bytes).unwrap();
                zip.finish().unwrap();
            } else {
                fs::create_dir(&source).unwrap();
                fs::write(source.join("page.png"), &bytes).unwrap();
                fs::write(source.join("notes.txt"), "extra content").unwrap();
            }
            let output = root.join(format!("book-{archive}-{index}.epub"));
            if fail {
                fs::write(&output, "existing book").unwrap();
            }
            let flag = if archive { "--delete" } else { "-d" };
            let result = invoke(&source, &output, flag, answer, dry);
            assert_eq!(result.status.success(), !fail, "{result:?}");
            assert_eq!(!source.exists(), deleted);
            let prompt = String::from_utf8_lossy(&result.stderr);
            assert_eq!(prompt.contains("[y/N]"), !dry && !fail);
            if fail {
                assert_eq!(fs::read(&output).unwrap(), b"existing book");
            } else if dry {
                assert!(!output.exists());
            } else {
                let mut book = ZipArchive::new(fs::File::open(&output).unwrap()).unwrap();
                assert!(book.by_name("EPUB/images/1.png").is_ok());
            }
            if !archive && !deleted {
                assert_eq!(fs::read(source.join("page.png")).unwrap(), bytes);
                assert!(source.join("notes.txt").exists());
            }
        }
    }
    let source = root.join("inside");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("page.png"), &bytes).unwrap();
    let output = source.join("book.epub");
    let result = invoke(&source, &output, "-d", "y\n", false);
    assert!(result.status.success(), "{result:?}");
    assert!(source.join("page.png").exists() && output.exists());
    assert!(!String::from_utf8_lossy(&result.stderr).contains("[y/N]"));
    fs::remove_dir_all(root).unwrap();
}
