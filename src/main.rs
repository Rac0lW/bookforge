use chrono::{Local, NaiveDateTime, TimeZone};
use clap::{Parser, ValueEnum};
use std::{
    cmp::Ordering,
    collections::HashSet,
    error::Error,
    fs::{self, File, OpenOptions},
    io::{self, BufReader, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum Sort {
    Auto,
    Name,
    Time,
}

#[derive(Parser)]
#[command(
    version,
    about = "将 JPEG/PNG/静态 WebP 图片目录转换为 Apple Books 固定版式 EPUB"
)]
struct Args {
    /// 图片目录（不递归）
    directory: PathBuf,
    /// 输出文件名或路径；与 --desktop 同用时只能是文件名
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// 输出到系统桌面目录（未指定 --output 时默认启用）
    #[arg(short, long)]
    desktop: bool,
    /// auto 优先统一序号，否则尝试时间；name 为自然排序；time 优先 EXIF 拍摄时间
    #[arg(long, value_enum, default_value = "auto")]
    sort: Sort,
    /// 显示顺序、封面和输出路径，不写文件
    #[arg(long)]
    dry_run: bool,
}

fn output_path(title: &str, output: Option<&Path>, desktop: Option<&Path>) -> Result<PathBuf> {
    let mut output = output
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(format!("{title}.epub")));
    if output.file_name().is_none() || output.as_os_str().is_empty() {
        return Err("输出必须是文件名或文件路径".into());
    }
    if let Some(desktop) = desktop {
        if output.as_os_str() != output.file_name().unwrap() {
            return Err("--desktop 与 --output 同用时，--output 只能是文件名，不能包含目录".into());
        }
        output = desktop.join(output);
    }
    if output.extension().is_none() {
        output.set_extension("epub");
    }
    Ok(output)
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.as_bytes(), b.as_bytes());
    while !a.is_empty() && !b.is_empty() {
        if a[0].is_ascii_digit() && b[0].is_ascii_digit() {
            let an = a.iter().take_while(|c| c.is_ascii_digit()).count();
            let bn = b.iter().take_while(|c| c.is_ascii_digit()).count();
            let av = &a[..an];
            let bv = &b[..bn];
            let av = &av[av.iter().take_while(|&&c| c == b'0').count()..];
            let bv = &bv[bv.iter().take_while(|&&c| c == b'0').count()..];
            let order = av.len().cmp(&bv.len()).then_with(|| av.cmp(bv));
            if order != Ordering::Equal {
                return order;
            }
            a = &a[an..];
            b = &b[bn..];
        } else {
            let order = a[0].cmp(&b[0]);
            if order != Ordering::Equal {
                return order;
            }
            a = &a[1..];
            b = &b[1..];
        }
    }
    a.len().cmp(&b.len())
}

struct Picture {
    path: PathBuf,
    extension: &'static str,
    media_type: &'static str,
    width: u32,
    height: u32,
    convert_png: bool,
    timestamp: Option<(SystemTime, bool)>, // true: EXIF, false: modification time
}

// ponytail: only a single numeric run with a shared prefix/suffix counts as a sequence;
// use --sort name for chapter/page names with multiple numeric runs.
fn sequence_parts(path: &Path) -> Option<(&str, &str, &str)> {
    let name = path.file_stem()?.to_str()?;
    let start = name.find(|c: char| c.is_ascii_digit())?;
    let end = start + name[start..].bytes().take_while(u8::is_ascii_digit).count();
    if name[end..].bytes().any(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((&name[..start], &name[start..end], &name[end..]))
}

fn numbered_sequence(pictures: &[Picture]) -> bool {
    let Some(first) = pictures.first().and_then(|p| sequence_parts(&p.path)) else {
        return false;
    };
    let mut numbers = HashSet::new();
    pictures.iter().all(|p| {
        sequence_parts(&p.path).is_some_and(|(prefix, number, suffix)| {
            prefix == first.0 && suffix == first.2 && numbers.insert(number.trim_start_matches('0'))
        })
    })
}

fn exif_text(exif: &exif::Exif, tag: exif::Tag) -> Option<&str> {
    match &exif.get_field(tag, exif::In::PRIMARY)?.value {
        exif::Value::Ascii(values) => std::str::from_utf8(values.first()?).ok(),
        _ => None,
    }
}

fn capture_time(date: &str, offset: Option<&str>, subsecond: Option<&str>) -> Option<SystemTime> {
    let date = if let Some(subsecond) = subsecond {
        if subsecond.is_empty() || !subsecond.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        format!("{date}.{}", &subsecond[..subsecond.len().min(9)])
    } else {
        date.to_owned()
    };
    let date = NaiveDateTime::parse_from_str(&date, "%Y:%m:%d %H:%M:%S%.f").ok()?;
    if let Some(offset) = offset {
        let offset: chrono::FixedOffset = offset.parse().ok()?;
        Some(offset.from_local_datetime(&date).single()?.into())
    } else {
        // A missing EXIF timezone means local time; DST ambiguity falls back to mtime.
        Some(Local.from_local_datetime(&date).single()?.into())
    }
}

fn picture_time(path: &Path) -> Result<(SystemTime, bool)> {
    let mut file = BufReader::new(File::open(path)?);
    if let Ok(exif) = exif::Reader::new().read_from_container(&mut file)
        && let Some(date) = exif_text(&exif, exif::Tag::DateTimeOriginal)
        && let Some(timestamp) = capture_time(
            date,
            exif_text(&exif, exif::Tag::OffsetTimeOriginal),
            exif_text(&exif, exif::Tag::SubSecTimeOriginal),
        )
    {
        return Ok((timestamp, true));
    }
    Ok((fs::metadata(path)?.modified()?, false))
}

fn sort_pictures(pictures: &mut [Picture], sort: Sort) -> Result<String> {
    pictures.sort_by(|a, b| {
        let an = a.path.file_name().unwrap().to_string_lossy();
        let bn = b.path.file_name().unwrap().to_string_lossy();
        natural_cmp(&an, &bn).then_with(|| a.path.cmp(&b.path))
    });
    if sort == Sort::Name {
        return Ok("name：文件名自然排序".into());
    }
    if sort == Sort::Auto && (pictures.len() < 2 || numbered_sequence(pictures)) {
        return Ok("auto → name：单张图片或统一且不重复的文件名序号".into());
    }
    for picture in pictures.iter_mut() {
        picture.timestamp = Some(picture_time(&picture.path)?);
    }
    let distinct: HashSet<_> = pictures.iter().map(|p| p.timestamp.unwrap().0).collect();
    let exif_count = pictures.iter().filter(|p| p.timestamp.unwrap().1).count();
    let sources = format!(
        "EXIF {exif_count} 张，修改时间 {} 张；无时区 EXIF 按本机时区解释",
        pictures.len() - exif_count
    );
    if sort == Sort::Auto && distinct.len() != pictures.len() {
        return Ok(format!(
            "auto → name：无可靠序号且存在相同时间，退回自然排序（{sources}）；请检查预览"
        ));
    }
    // Stable sort retains natural filename ordering for equal timestamps.
    pictures.sort_by_key(|p| p.timestamp.unwrap().0);
    Ok(format!(
        "{}time：从早到晚，同时间按自然名称排序（{sources}）{}",
        if sort == Sort::Auto { "auto → " } else { "" },
        if sort == Sort::Auto {
            "；时间可能因复制或编辑改变，请检查预览"
        } else {
            ""
        }
    ))
}

fn decode_picture(path: &Path) -> Result<(image::DynamicImage, image::ImageFormat)> {
    let reader = image::ImageReader::open(path)?.with_guessed_format()?;
    let format = reader
        .format()
        .ok_or_else(|| format!("无法识别图片格式：{}", path.display()))?;
    if format == image::ImageFormat::WebP {
        let decoder = image::codecs::webp::WebPDecoder::new(BufReader::new(File::open(path)?))
            .map_err(|e| format!("图片无法解码 {}: {e}", path.display()))?;
        if decoder.has_animation() {
            return Err(format!("不支持动态 WebP：{}；请先导出为静态图片", path.display()).into());
        }
    }
    let decoded = reader
        .decode()
        .map_err(|e| format!("图片无法解码 {}: {e}", path.display()))?;
    Ok((decoded, format))
}

fn pictures(directory: &Path) -> Result<Vec<Picture>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if matches!(extension.as_str(), "jpg" | "jpeg" | "png" | "webp") {
            paths.push(path);
        }
    }
    if paths.is_empty() {
        return Err("目录中没有 JPEG、PNG 或 WebP 图片".into());
    }
    paths
        .into_iter()
        .map(|path| {
            // Validate before creating output; JPEG/PNG retain their original bytes.
            let (decoded, format) = decode_picture(&path)?;
            let (extension, media_type) = match format {
                image::ImageFormat::Jpeg => ("jpg", "image/jpeg"),
                image::ImageFormat::Png | image::ImageFormat::WebP => ("png", "image/png"),
                _ => return Err(format!("不支持的图片格式：{}", path.display()).into()),
            };
            Ok(Picture {
                width: decoded.width(),
                height: decoded.height(),
                convert_png: format == image::ImageFormat::WebP,
                path,
                extension,
                media_type,
                timestamp: None,
            })
        })
        .collect()
}

fn add(zip: &mut ZipWriter<&mut File>, name: &str, content: &str) -> Result<()> {
    zip.start_file(
        name,
        SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
    )?;
    zip.write_all(content.as_bytes())?;
    Ok(())
}

fn package(file: &mut File, title: &str, pictures: &[Picture]) -> Result<()> {
    let mut zip = ZipWriter::new(file);
    add(&mut zip, "mimetype", "application/epub+zip")?;
    add(
        &mut zip,
        "META-INF/container.xml",
        r#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="EPUB/package.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
    )?;
    let title = escape(title);
    let mut manifest = String::from(
        r#"<item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>"#,
    );
    let mut spine = String::new();
    let mut toc = String::new();
    for (index, picture) in pictures.iter().enumerate() {
        let n = index + 1;
        let image_name = format!("images/{n}.{}", picture.extension);
        let cover = if index == 0 {
            " properties=\"cover-image\""
        } else {
            ""
        };
        manifest.push_str(&format!(r#"<item id="image{n}" href="{image_name}" media-type="{}"{cover}/><item id="page{n}" href="pages/{n}.xhtml" media-type="application/xhtml+xml"/>"#, picture.media_type));
        spine.push_str(&format!(r#"<itemref idref="page{n}"/>"#));
        toc.push_str(&format!(
            r#"<li><a href="pages/{n}.xhtml">第 {n} 页</a></li>"#
        ));
        let (w, h) = (picture.width, picture.height);
        add(
            &mut zip,
            &format!("EPUB/pages/{n}.xhtml"),
            &format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" lang="zh" xml:lang="zh"><head><title>第 {n} 页</title><meta name="viewport" content="width={w}, height={h}"/><style>html,body{{margin:0;padding:0;width:{w}px;height:{h}px;}}img{{display:block;width:{w}px;height:{h}px;}}</style></head><body><img src="../{image_name}" alt="第 {n} 页"/></body></html>"#
            ),
        )?;
        zip.start_file(
            format!("EPUB/{image_name}"),
            SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
        )?;
        if picture.convert_png {
            // Decode one page at a time rather than retaining the whole book in memory.
            let (decoded, _) = decode_picture(&picture.path)?;
            decoded.write_with_encoder(image::codecs::png::PngEncoder::new(&mut zip))?;
        } else {
            io::copy(&mut File::open(&picture.path)?, &mut zip)?;
        }
    }
    add(
        &mut zip,
        "EPUB/nav.xhtml",
        &format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" lang="zh" xml:lang="zh"><head><title>{title}</title></head><body><nav epub:type="toc"><h1>目录</h1><ol>{toc}</ol></nav><nav epub:type="landmarks"><h2>阅读起点</h2><ol><li><a epub:type="bodymatter" href="pages/1.xhtml">正文</a></li></ol></nav></body></html>"#
        ),
    )?;
    let identifier = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let modified = time::OffsetDateTime::now_utc()
        .replace_nanosecond(0)?
        .format(&time::format_description::well_known::Rfc3339)?;
    add(
        &mut zip,
        "EPUB/package.opf",
        &format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="book-id" prefix="rendition: http://www.idpf.org/vocab/rendition/#"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="book-id">urn:bookforge:{identifier}</dc:identifier><dc:title>{title}</dc:title><dc:language>zh</dc:language><meta property="dcterms:modified">{modified}</meta><meta property="rendition:layout">pre-paginated</meta><meta property="rendition:spread">none</meta></metadata><manifest>{manifest}</manifest><spine page-progression-direction="ltr">{spine}</spine></package>"#
        ),
    )?;
    zip.finish()?;
    Ok(())
}

fn run() -> Result<()> {
    let args = Args::parse();
    let directory = args.directory.canonicalize()?;
    let title = directory
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("book");
    let desktop = if args.desktop || args.output.is_none() {
        Some(dirs::desktop_dir().ok_or("无法定位系统桌面目录，请使用 --output 指定路径")?)
    } else {
        None
    };
    let output = output_path(title, args.output.as_deref(), desktop.as_deref())?;
    let mut pictures = pictures(&directory)?;
    println!("排序：{}", sort_pictures(&mut pictures, args.sort)?);
    for (i, picture) in pictures.iter().enumerate() {
        let timestamp = picture
            .timestamp
            .map(|(t, exif)| {
                format!(
                    " [{} {}]",
                    if exif { "EXIF" } else { "mtime" },
                    chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339()
                )
            })
            .unwrap_or_default();
        println!(
            "{}: {}{}{}{}",
            i + 1,
            picture.path.display(),
            if i == 0 { " [封面]" } else { "" },
            if picture.convert_png {
                " [WebP → PNG]"
            } else {
                ""
            },
            timestamp
        );
    }
    println!("输出：{}", output.display());
    if args.dry_run {
        println!("仅预览，未写入文件。");
        return Ok(());
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output)?;
    let result = package(&mut file, title, &pictures).and_then(|_| {
        file.sync_all()?;
        Ok(())
    });
    drop(file);
    if let Err(error) = result {
        if let Err(cleanup) = fs::remove_file(&output) {
            eprintln!("无法删除不完整输出 {}: {cleanup}", output.display());
        }
        return Err(error);
    }
    println!("已生成：{}（{} 页）", output.display(), pictures.len());
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("错误：{error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordering_and_metadata() {
        let mut names = ["10.png", "2.png", "1.png", "0002.png"];
        names.sort_by(|a, b| natural_cmp(a, b).then_with(|| a.cmp(b)));
        assert_eq!(names, ["1.png", "0002.png", "2.png", "10.png"]);
        assert_eq!(
            natural_cmp("999999999999999999999999", "1000000000000000000000000"),
            Ordering::Less
        );
        assert_eq!(escape("A&B<\"'>"), "A&amp;B&lt;&quot;&apos;&gt;");
    }

    #[test]
    fn options_paths_and_capture_times() {
        let args = Args::try_parse_from([
            "bookforge",
            "--sort=time",
            "-d",
            "images",
            "--dry-run",
            "-o",
            "书",
        ])
        .unwrap();
        assert!(args.desktop && args.dry_run);
        assert_eq!(args.sort, Sort::Time);
        assert!(Args::try_parse_from(["bookforge", "images", "--sort", "guess"]).is_err());
        assert!(Args::try_parse_from(["bookforge", "images", "-o"]).is_err());
        let desktop = Path::new("/test/Desktop");
        assert_eq!(
            output_path("书", None, Some(desktop)).unwrap(),
            desktop.join("书.epub")
        );
        assert_eq!(
            output_path("书", Some(Path::new("漫画")), Some(desktop)).unwrap(),
            desktop.join("漫画.epub")
        );
        assert_eq!(
            output_path("书", Some(Path::new("other/漫画.epub")), None).unwrap(),
            Path::new("other/漫画.epub")
        );
        for name in ["../书", "/tmp/书", "./书", "", ".", ".."] {
            assert!(output_path("书", Some(Path::new(name)), Some(desktop)).is_err());
        }
        let expected = UNIX_EPOCH + std::time::Duration::new(1, 123_000_000);
        assert_eq!(
            capture_time("1970:01:01 08:00:01", Some("+08:00"), Some("123")),
            Some(expected)
        );
        assert_eq!(
            capture_time("1970:01:01 00:00:01", Some("+00:00"), None),
            Some(UNIX_EPOCH + std::time::Duration::from_secs(1))
        );
        assert!(capture_time("2025:02:30 00:00:00", Some("+00:00"), None).is_none());
        assert!(capture_time("2025:01:01 00:00:00", Some("bad"), None).is_none());
        assert!(capture_time("2025:01:01 00:00:00", Some("+00:00"), Some("bad")).is_none());
    }

    #[test]
    fn conservative_sequence_detection() {
        for (names, expected) in [
            (["页1.png", "页02.jpg", "页10.png"], true),
            (["页1.png", "页01.png", "页2.png"], false),
            (["a1.png", "b2.png", "c3.png"], false),
            (["ch1p1.png", "ch1p2.png", "ch1p3.png"], false),
            (["a.png", "b.png", "c.png"], false),
        ] {
            let pictures: Vec<_> = names
                .iter()
                .map(|name| Picture {
                    path: PathBuf::from(name),
                    extension: "png",
                    media_type: "image/png",
                    width: 1,
                    height: 1,
                    convert_png: false,
                    timestamp: None,
                })
                .collect();
            assert_eq!(numbered_sequence(&pictures), expected, "{names:?}");
        }
    }
}
