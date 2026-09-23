use std::{
    fs,
    io::Read,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

#[test]
fn zip_images_in_path_order_and_safe_extraction() {
    let root = std::env::temp_dir().join(format!(
        "bookforge-zip-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let archive = root.join("comic.ZIP");
    let output = root.join("comic.epub");
    let bytes: Vec<_> = (1..=3)
        .map(|n| {
            let png = root.join(format!("page{n}.png"));
            image::RgbaImage::from_pixel(2, 3, image::Rgba([n, 6, 7, 255]))
                .save(&png)
                .unwrap();
            fs::read(&png).unwrap()
        })
        .collect();
    let make_zip = |items: &[(&str, &[u8])]| {
        let mut zip = ZipWriter::new(fs::File::create(&archive).unwrap());
        for (name, data) in items {
            zip.start_file(
                *name,
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .unwrap();
            use std::io::Write;
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
    };
    let invoke = |dry: bool| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_bookforge"));
        command.arg(&archive).arg("-o").arg(&output);
        if dry {
            command.arg("--dry-run");
        }
        command.output().unwrap()
    };
    make_zip(&[
        ("b/10.PNG", &bytes[2]),
        ("ignored.txt", b"not a picture"),
        ("a/2.png", &bytes[1]),
        ("a/1.png", &bytes[0]),
    ]);
    let preview = invoke(true);
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    let text = String::from_utf8_lossy(&preview.stdout).replace('\\', "/");
    assert!(text.find("a/1.png").unwrap() < text.find("a/2.png").unwrap());
    assert!(text.find("a/2.png").unwrap() < text.find("b/10.PNG").unwrap());
    assert!(text.contains("a/1.png [封面]"));
    assert!(!output.exists());
    let result = invoke(false);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let log = String::from_utf8_lossy(&result.stdout);
    assert!(log.contains("3 页，封面：") && log.contains("已生成："));
    assert!(!log.lines().any(|line| line.starts_with("1: ")));
    assert!(result.stderr.is_empty());
    let mut book = ZipArchive::new(fs::File::open(&output).unwrap()).unwrap();
    for n in 1..=3 {
        let mut page = Vec::new();
        book.by_name(&format!("EPUB/images/{n}.png"))
            .unwrap()
            .read_to_end(&mut page)
            .unwrap();
        assert_eq!(page, bytes[n - 1]);
    }
    drop(book);
    let existing = fs::read(&output).unwrap();
    assert!(!invoke(false).status.success());
    assert_eq!(fs::read(&output).unwrap(), existing);
    fs::remove_file(&output).unwrap();
    make_zip(&[("../escape.png", &bytes[0])]);
    let rejected = invoke(false);
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("不安全"));
    assert!(!output.exists());
    fs::remove_dir_all(&root).unwrap();
}
