use std::{
    fs,
    io::Read,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
use zip::ZipArchive;

#[test]
fn spreads_and_reading_direction() {
    let root = std::env::temp_dir().join(format!(
        "bookforge-spread-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let pictures = root.join("まんが");
    fs::create_dir_all(&pictures).unwrap();
    for (n, width) in [(1, 2), (2, 2), (3, 4), (4, 2), (5, 2)] {
        image::RgbaImage::from_pixel(width, 3, image::Rgba([n, 0, 0, 255]))
            .save(pictures.join(format!("{n}.png")))
            .unwrap();
    }
    let chinese = root.join("漫画");
    fs::create_dir(&chinese).unwrap();
    for n in 1..=5 {
        fs::copy(
            pictures.join(format!("{n}.png")),
            chinese.join(format!("{n}.png")),
        )
        .unwrap();
    }
    for (source, flag, direction, sides) in [
        (
            &pictures,
            None,
            "rtl",
            ["left", "right", "rendition:spread-none", "right", "left"],
        ),
        (
            &pictures,
            Some("--l2r"),
            "ltr",
            ["right", "left", "rendition:spread-none", "left", "right"],
        ),
        (
            &chinese,
            Some("--r2l"),
            "rtl",
            ["left", "right", "rendition:spread-none", "right", "left"],
        ),
        (
            &chinese,
            None,
            "ltr",
            ["right", "left", "rendition:spread-none", "left", "right"],
        ),
    ] {
        let output = root.join(format!(
            "{}-{direction}-{}.epub",
            source.file_name().unwrap().to_string_lossy(),
            flag.unwrap_or("auto")
        ));
        let mut command = Command::new(env!("CARGO_BIN_EXE_bookforge"));
        command.arg(source).arg("-o").arg(&output);
        if let Some(flag) = flag {
            command.arg(flag);
        }
        let result = command.output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let mut zip = ZipArchive::new(fs::File::open(output).unwrap()).unwrap();
        let mut opf = String::new();
        zip.by_name("EPUB/package.opf")
            .unwrap()
            .read_to_string(&mut opf)
            .unwrap();
        assert!(opf.contains("<meta property=\"rendition:spread\">landscape</meta>"));
        assert!(opf.contains(&format!("page-progression-direction=\"{direction}\"")));
        for (index, side) in sides.iter().enumerate() {
            let property = if side.starts_with("rendition:") {
                side.to_string()
            } else {
                format!("page-spread-{side}")
            };
            assert!(opf.contains(&format!(
                "<itemref idref=\"page{}\" properties=\"{property}\"/>",
                index + 1
            )));
        }
    }
    // Apple Books needs a book-level single-page setting for an all-landscape book.
    let landscape = root.join("横图");
    fs::create_dir(&landscape).unwrap();
    for n in 1..=2 {
        image::RgbaImage::from_pixel(1280, 905, image::Rgba([n, 0, 0, 255]))
            .save(landscape.join(format!("{n}.png")))
            .unwrap();
    }
    let output = root.join("landscape.epub");
    assert!(
        Command::new(env!("CARGO_BIN_EXE_bookforge"))
            .arg(&landscape)
            .arg("-o")
            .arg(&output)
            .status()
            .unwrap()
            .success()
    );
    let mut zip = ZipArchive::new(fs::File::open(output).unwrap()).unwrap();
    let mut opf = String::new();
    zip.by_name("EPUB/package.opf")
        .unwrap()
        .read_to_string(&mut opf)
        .unwrap();
    assert!(opf.contains("<meta property=\"rendition:spread\">none</meta>"));
    assert_eq!(
        opf.matches("properties=\"rendition:spread-none\"").count(),
        2
    );

    let conflict = Command::new(env!("CARGO_BIN_EXE_bookforge"))
        .arg(&pictures)
        .args(["--r2l", "--l2r", "--dry-run"])
        .output()
        .unwrap();
    assert!(!conflict.status.success());
    fs::remove_dir_all(root).unwrap();
}
