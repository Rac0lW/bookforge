use std::{
    fs,
    io::Read,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
use zip::ZipArchive;

#[test]
fn portrait_single_pages_and_reading_direction() {
    let root = std::env::temp_dir().join(format!(
        "bookforge-single-page-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let japanese = root.join("まんが");
    fs::create_dir_all(&japanese).unwrap();
    for (n, width, height) in [(1, 2, 3), (2, 4, 3), (3, 3, 3)] {
        image::RgbaImage::from_pixel(width, height, image::Rgba([n, 0, 0, 255]))
            .save(japanese.join(format!("{n}.png")))
            .unwrap();
    }
    let chinese = root.join("漫画");
    fs::create_dir(&chinese).unwrap();
    for n in 1..=3 {
        fs::copy(
            japanese.join(format!("{n}.png")),
            chinese.join(format!("{n}.png")),
        )
        .unwrap();
    }
    for (source, flag, direction) in [
        (&japanese, None, "rtl"),
        (&japanese, Some("--l2r"), "ltr"),
        (&chinese, Some("--r2l"), "rtl"),
        (&chinese, None, "ltr"),
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
        assert!(String::from_utf8_lossy(&result.stdout).contains("竖屏单页"));
        let mut zip = ZipArchive::new(fs::File::open(output).unwrap()).unwrap();
        let mut opf = String::new();
        zip.by_name("EPUB/package.opf")
            .unwrap()
            .read_to_string(&mut opf)
            .unwrap();
        assert!(opf.contains("<meta property=\"rendition:spread\">none</meta>"));
        assert!(opf.contains(&format!("page-progression-direction=\"{direction}\"")));
        assert_eq!(
            opf.matches("properties=\"rendition:spread-none\"").count(),
            3
        );
        assert!(!opf.contains("page-spread-") && !opf.contains("landscape"));
        for (n, viewport) in [
            (1, "width=2, height=3"),
            (2, "width=4, height=6"),
            (3, "width=3, height=5"),
        ] {
            let mut page = String::new();
            zip.by_name(&format!("EPUB/pages/{n}.xhtml"))
                .unwrap()
                .read_to_string(&mut page)
                .unwrap();
            assert!(page.contains(viewport), "{page}");
            assert!(page.contains("object-fit:contain"));
        }
    }
    // An all-wide book must use the same single-page layout as a mixed book.
    let wide = root.join("横图");
    fs::create_dir(&wide).unwrap();
    fs::copy(japanese.join("2.png"), wide.join("1.png")).unwrap();
    let output = root.join("wide.epub");
    let result = Command::new(env!("CARGO_BIN_EXE_bookforge"))
        .arg(&wide)
        .arg("-o")
        .arg(&output)
        .output()
        .unwrap();
    assert!(result.status.success());
    let mut zip = ZipArchive::new(fs::File::open(output).unwrap()).unwrap();
    let mut opf = String::new();
    zip.by_name("EPUB/package.opf")
        .unwrap()
        .read_to_string(&mut opf)
        .unwrap();
    assert!(opf.contains("<meta property=\"rendition:spread\">none</meta>"));
    assert!(opf.contains("properties=\"rendition:spread-none\""));
    let mut page = String::new();
    zip.by_name("EPUB/pages/1.xhtml")
        .unwrap()
        .read_to_string(&mut page)
        .unwrap();
    assert!(page.contains("width=4, height=6"));

    let conflict = Command::new(env!("CARGO_BIN_EXE_bookforge"))
        .arg(&japanese)
        .args(["--r2l", "--l2r", "--dry-run"])
        .output()
        .unwrap();
    assert!(!conflict.status.success());
    fs::remove_dir_all(root).unwrap();
}
