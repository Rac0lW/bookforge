use std::{
    fs,
    io::Read,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn webp_conversion_and_rejection() {
    let root = std::env::temp_dir().join(format!(
        "bookforge-webp-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let input = root.join("images");
    fs::create_dir_all(&input).unwrap();
    let original = image::RgbaImage::from_raw(2, 1, vec![255, 0, 0, 255, 0, 255, 0, 128]).unwrap();
    let webp = input.join("1.WeBp");
    original
        .save_with_format(&webp, image::ImageFormat::WebP)
        .unwrap();
    let webp_bytes = fs::read(&webp).unwrap();
    original.save(input.join("2.png")).unwrap();
    let rgb = image::DynamicImage::ImageRgba8(original.clone()).to_rgb8();
    rgb.save(input.join("10.jpg")).unwrap();
    let output = root.join("webp.epub");
    let invoke = |extra: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_bookforge"))
            .arg(&input)
            .arg("-o")
            .arg(&output)
            .args(extra)
            .output()
            .unwrap()
    };
    let preview = invoke(&["--dry-run"]);
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    assert!(String::from_utf8_lossy(&preview.stdout).contains("[封面] [WebP → PNG]"));
    assert!(!output.exists());
    let result = invoke(&[]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(fs::read(&webp).unwrap(), webp_bytes);
    let mut archive = zip::ZipArchive::new(fs::File::open(&output).unwrap()).unwrap();
    assert!(!archive.file_names().any(|name| name.ends_with(".webp")));
    let mut converted = Vec::new();
    archive
        .by_name("EPUB/images/1.png")
        .unwrap()
        .read_to_end(&mut converted)
        .unwrap();
    let decoded = image::load_from_memory_with_format(&converted, image::ImageFormat::Png)
        .unwrap()
        .to_rgba8();
    assert_eq!(decoded, original); // dimensions, colors and partial transparency survive
    for (embedded, source) in [("2.png", "2.png"), ("3.jpg", "10.jpg")] {
        let mut bytes = Vec::new();
        archive
            .by_name(&format!("EPUB/images/{embedded}"))
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(bytes, fs::read(input.join(source)).unwrap());
    }
    let mut opf = String::new();
    archive
        .by_name("EPUB/package.opf")
        .unwrap()
        .read_to_string(&mut opf)
        .unwrap();
    assert!(
        opf.contains("href=\"images/1.png\" media-type=\"image/png\" properties=\"cover-image\"")
    );
    drop(archive);

    // Two-frame animated WebP assembled from the encoder's static frame data.
    fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut bytes = kind.to_vec();
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(data);
        if !data.len().is_multiple_of(2) {
            bytes.push(0);
        }
        bytes
    }
    let mut animated = b"WEBP".to_vec();
    animated.extend(chunk(b"VP8X", &[0x12, 0, 0, 0, 1, 0, 0, 0, 0, 0]));
    animated.extend(chunk(b"ANIM", &[0; 6]));
    let mut frame = vec![0; 16];
    frame[6] = 1; // width minus one
    frame[12] = 100; // duration in milliseconds
    frame[15] = 2; // no blending
    frame.extend_from_slice(&webp_bytes[12..]);
    animated.extend(chunk(b"ANMF", &frame));
    animated.extend(chunk(b"ANMF", &frame));
    let mut riff = b"RIFF".to_vec();
    riff.extend_from_slice(&(animated.len() as u32).to_le_bytes());
    riff.extend(animated);
    // Verify this really is an animation, not merely a corrupted test fixture.
    use image::AnimationDecoder;
    let decoder = image::codecs::webp::WebPDecoder::new(std::io::Cursor::new(&riff)).unwrap();
    assert!(decoder.has_animation());
    assert_eq!(decoder.into_frames().collect_frames().unwrap().len(), 2);
    let invalid = input.join("4.webp");
    let untouched = fs::read(&output).unwrap();
    fs::write(&invalid, &riff).unwrap();
    for extra in [&["--dry-run"][..], &[][..]] {
        let result = invoke(extra);
        assert!(!result.status.success());
        let error = String::from_utf8_lossy(&result.stderr);
        assert!(
            error.contains("不支持动态 WebP") && error.contains("4.webp"),
            "{error}"
        );
    }
    let rejected = root.join("rejected.epub");
    assert!(
        !Command::new(env!("CARGO_BIN_EXE_bookforge"))
            .arg(&input)
            .arg("-o")
            .arg(&rejected)
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(!rejected.exists());
    fs::remove_file(&invalid).unwrap();
    // Detect by content, not just filename, so renaming cannot bypass rejection.
    fs::write(input.join("4.png"), &riff).unwrap();
    let result = invoke(&["--dry-run"]);
    assert!(String::from_utf8_lossy(&result.stderr).contains("不支持动态 WebP"));
    fs::remove_file(input.join("4.png")).unwrap();
    fs::write(&invalid, b"broken WebP").unwrap();
    assert!(!invoke(&["--dry-run"]).status.success());
    assert!(!invoke(&[]).status.success());
    assert_eq!(fs::read(&output).unwrap(), untouched);
    // Keep the successful EPUB for optional EPUBCheck / Apple Books inspection.
    println!("WebP sample: {}", output.display());
}
