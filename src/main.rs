use chrono::{Local, NaiveDate, NaiveDateTime, TimeZone};
use clap::{Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use std::process::Command;
use std::{
    cmp::Ordering,
    collections::HashSet,
    error::Error,
    fs::{self, File, OpenOptions},
    io::{self, BufReader, IsTerminal, Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering as AtomicOrdering},
        mpsc,
    },
    thread,
    time::{SystemTime, UNIX_EPOCH},
};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

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
    about = "将图片目录、ZIP 压缩包或链接转换为 Apple Books 固定版式 EPUB"
)]
struct Args {
    /// 图片目录、ZIP 压缩包或 HTTP(S) 图片链接
    directory: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<ConfigCommand>,
    /// 输出文件名或路径；与 --desktop 同用时只能是文件名
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// 强制输出到系统桌面目录（默认位置可通过配置修改）
    #[arg(short, long)]
    desktop: bool,
    /// 生成后在 macOS 中尝试用 Books 打开 EPUB 并导入书库
    #[arg(long, conflicts_with = "no_books")]
    books: bool,
    /// 本次不打开 Books（覆盖配置文件）
    #[arg(long)]
    no_books: bool,
    /// auto 优先统一序号，否则尝试时间；name 为自然排序；time 优先 EXIF 拍摄时间
    #[arg(long, value_enum, default_value = "auto")]
    sort: Sort,
    /// 从右向左翻页（日漫）；覆盖标题自动判断
    #[arg(long, conflicts_with = "l2r")]
    r2l: bool,
    /// 从左向右翻页；覆盖标题自动判断
    #[arg(long)]
    l2r: bool,
    /// 显示顺序、封面和输出路径，不写文件
    #[arg(long)]
    dry_run: bool,
}

#[derive(Subcommand)]
enum ConfigCommand {
    /// 创建默认配置文件（不会覆盖已有文件）
    Init,
    /// 使用系统默认应用打开配置文件
    Open,
    /// 修改配置
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// 设置 books、desktop 或 output_dir
    Set { key: String, value: String },
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Config {
    books: bool,
    // None retains the original desktop default.
    desktop: Option<bool>,
    output_dir: Option<PathBuf>,
}

impl Config {
    fn load(path: &Path) -> Result<Self> {
        match fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text)
                .map_err(|e| format!("配置文件 {} 无效：{e}", path.display()).into()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("读取配置文件 {} 失败：{e}", path.display()).into()),
        }
    }

    fn set(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "books" => self.books = value.parse().map_err(|_| "books 必须是 true 或 false")?,
            "desktop" => {
                let enabled = value.parse().map_err(|_| "desktop 必须是 true 或 false")?;
                self.desktop = Some(enabled);
                self.output_dir = None;
            }
            "output_dir" => {
                if value.is_empty() {
                    return Err("output_dir 不能为空".into());
                }
                self.output_dir = Some(PathBuf::from(value));
                self.desktop = Some(false);
            }
            _ => return Err("未知配置项；可用：books、desktop、output_dir".into()),
        }
        Ok(())
    }
}

fn config_path() -> Result<PathBuf> {
    Ok(dirs::home_dir()
        .ok_or("无法定位用户主目录")?
        .join(".config/bookforge/config.toml"))
}

fn config_command(path: &Path, command: ConfigCommand) -> Result<()> {
    match command {
        ConfigCommand::Open => {
            if !path.is_file() {
                return Err(format!(
                    "配置文件不存在：{}；请先运行 bookforge init",
                    path.display()
                )
                .into());
            }
            #[cfg(target_os = "macos")]
            let opener = "open";
            #[cfg(target_os = "windows")]
            let opener = "explorer";
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            let opener = "xdg-open";
            let status = Command::new(opener).arg(path).status()?;
            if !status.success() {
                return Err(format!("无法用默认应用打开 {}（{status}）", path.display()).into());
            }
        }
        ConfigCommand::Init => {
            fs::create_dir_all(path.parent().unwrap())?;
            let mut file = match OpenOptions::new().write(true).create_new(true).open(path) {
                Ok(file) => file,
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    return Err(format!(
                        "配置已存在：{}；未覆盖，请使用 config set 修改",
                        path.display()
                    )
                    .into());
                }
                Err(e) => return Err(e.into()),
            };
            if let Err(error) =
                file.write_all(b"# bookforge defaults\nbooks = false\ndesktop = true\n")
            {
                let _ = fs::remove_file(path);
                return Err(error.into());
            }
        }
        ConfigCommand::Config {
            action: ConfigAction::Set { key, value },
        } => {
            let mut config = Config::load(path)?;
            config.set(&key, &value)?;
            fs::create_dir_all(path.parent().unwrap())?;
            // Write to a sibling first so a failed write cannot truncate the existing config.
            let temp = path.with_extension(format!(
                "toml.{}.{}.tmp",
                std::process::id(),
                SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
            ));
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)?;
            let result = (|| -> Result<()> {
                file.write_all(toml::to_string_pretty(&config)?.as_bytes())?;
                file.sync_all()?;
                fs::rename(&temp, path)?;
                Ok(())
            })();
            if result.is_err() {
                let _ = fs::remove_file(&temp);
            }
            result?;
        }
    }
    println!("配置：{}", path.display());
    Ok(())
}

fn configured_output(title: &str, args: &Args, config: &Config) -> Result<PathBuf> {
    let desktop = if args.desktop
        || (args.output.is_none() && config.output_dir.is_none() && config.desktop.unwrap_or(true))
    {
        Some(dirs::desktop_dir().ok_or("无法定位系统桌面目录，请使用 --output 指定路径")?)
    } else {
        None
    };
    let output = if !args.desktop && args.output.is_none() {
        config
            .output_dir
            .as_ref()
            .map(|dir| -> Result<PathBuf> {
                let dir = if let Ok(suffix) = dir.strip_prefix("~") {
                    dirs::home_dir().ok_or("无法定位用户主目录")?.join(suffix)
                } else {
                    dir.clone()
                };
                Ok(dir.join(format!("{title}.epub")))
            })
            .transpose()?
    } else {
        None
    };
    output_path(
        title,
        args.output.as_deref().or(output.as_deref()),
        desktop.as_deref(),
    )
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

fn japanese_title(title: &str) -> bool {
    // Kanji alone cannot distinguish Japanese from Chinese; flags handle ambiguous titles.
    title
        .chars()
        .any(|c| matches!(c, '\u{3040}'..='\u{30ff}' | '\u{ff66}'..='\u{ff9f}'))
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

fn sort_pictures(
    pictures: &mut [Picture],
    sort: Sort,
    root: &Path,
    archive: bool,
) -> Result<String> {
    pictures.sort_by(|a, b| {
        let an = a.path.strip_prefix(root).unwrap().to_string_lossy();
        let bn = b.path.strip_prefix(root).unwrap().to_string_lossy();
        natural_cmp(&an, &bn).then_with(|| a.path.cmp(&b.path))
    });
    if sort == Sort::Name {
        return Ok("name：文件名自然排序".into());
    }
    if sort == Sort::Auto && archive {
        return Ok("auto → name：压缩包按内部路径自然排序（时间戳可能不可靠）".into());
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

fn is_picture(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        matches!(
            e.to_ascii_lowercase().as_str(),
            "jpg" | "jpeg" | "png" | "webp"
        )
    })
}

fn directory_paths(directory: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_file() && is_picture(&entry.path()) {
            paths.push(entry.path());
        }
    }
    Ok(paths)
}

struct Extracted(PathBuf);

fn temporary_directory() -> Result<Extracted> {
    loop {
        let candidate = std::env::temp_dir().join(format!(
            "bookforge-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(Extracted(candidate)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
}

fn url_title(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let name = path.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    let name: String = name
        .chars()
        .filter(|c| {
            !c.is_control() && !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        })
        .collect();
    if name.is_empty() || name == "." || name == ".." || name.contains('@') {
        "book".into()
    } else {
        name
    }
}

fn ensure_gallery_dl() -> Result<()> {
    match Command::new("gallery-dl").arg("--version").output() {
        Ok(result) if result.status.success() => return Ok(()),
        Ok(result) => return Err(format!("gallery-dl 无法运行（{}）", result.status).into()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("无法启动 gallery-dl：{error}").into()),
    }
    if !cfg!(target_os = "macos") || !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        return Err("未找到 gallery-dl；请先安装（macOS 可运行 brew install gallery-dl）".into());
    }
    eprint!("未找到 gallery-dl，是否使用 Homebrew 安装？[y/N] ");
    io::stderr().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        return Err("已取消安装 gallery-dl".into());
    }
    let status = Command::new("brew")
        .args(["install", "gallery-dl"])
        .status()
        .map_err(|e| format!("无法运行 brew：{e}；请手动安装 gallery-dl"))?;
    if !status.success() {
        return Err(format!("brew install gallery-dl 失败（{status}）").into());
    }
    if !Command::new("gallery-dl")
        .arg("--version")
        .status()?
        .success()
    {
        return Err("安装后 gallery-dl 仍无法运行".into());
    }
    Ok(())
}

fn download(url: &str) -> Result<(Extracted, Vec<PathBuf>)> {
    ensure_gallery_dl()?;
    let temp = temporary_directory()?;
    let status = Command::new("gallery-dl")
        .arg("-D")
        .arg(&temp.0)
        .arg("--")
        .arg(url)
        .status()?;
    if !status.success() {
        return Err(format!("gallery-dl 下载失败（{status}）").into());
    }
    let paths = directory_paths(&temp.0)?;
    Ok((temp, paths))
}

impl Drop for Extracted {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("无法清理临时图片 {}: {error}", self.0.display());
        }
    }
}

fn extract(archive: &Path, show_progress: bool) -> Result<(Extracted, Vec<PathBuf>)> {
    let mut zip = ZipArchive::new(File::open(archive)?)?;
    let root = temporary_directory()?;
    let mut paths = Vec::new();
    let mut total = 0u64;
    let fallback_time = fs::metadata(archive)?.modified()?;
    let entries = zip.len();
    if show_progress && entries > 0 {
        progress("解压", 0, entries);
    }
    for i in 0..entries {
        let mut entry = zip.by_index(i)?;
        if entry.is_dir() || !is_picture(Path::new(entry.name())) {
            if show_progress {
                progress("解压", i + 1, entries);
            }
            continue;
        }
        let relative = entry.enclosed_name().ok_or("ZIP 中存在不安全的图片路径")?;
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err("ZIP 中不能包含图片符号链接".into());
        }
        // ponytail: 512 MiB/image and 2 GiB/book cap decompression bombs; raise for larger books.
        if entry.size() > 512 * 1024 * 1024 || total + entry.size() > 2 * 1024 * 1024 * 1024 {
            return Err(format!("ZIP 图片过大：{}", entry.name()).into());
        }
        let path = root.0.join(relative);
        fs::create_dir_all(path.parent().unwrap())?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        let copied = io::copy(&mut entry.by_ref().take(512 * 1024 * 1024 + 1), &mut file)?;
        total += copied;
        if total > 2 * 1024 * 1024 * 1024 {
            return Err("ZIP 图片总大小超过 2 GiB".into());
        }
        let modified = entry
            .last_modified()
            .and_then(|date| {
                NaiveDate::from_ymd_opt(date.year().into(), date.month().into(), date.day().into())
                    .and_then(|day| {
                        day.and_hms_opt(
                            date.hour().into(),
                            date.minute().into(),
                            date.second().into(),
                        )
                    })
                    .and_then(|local| Local.from_local_datetime(&local).single())
                    .map(SystemTime::from)
            })
            .unwrap_or(fallback_time);
        file.set_modified(modified)?;
        paths.push(path);
        if show_progress {
            progress("解压", i + 1, entries);
        }
    }
    Ok((root, paths))
}

fn progress_bar(done: usize, total: usize) -> String {
    let filled = done * 20 / total;
    format!(
        "[{}{}] {done}/{total}",
        "#".repeat(filled),
        "-".repeat(20 - filled)
    )
}

fn progress(stage: &str, done: usize, total: usize) {
    if io::stderr().is_terminal() {
        eprint!("\r{stage} {}", progress_bar(done, total));
        let _ = io::stderr().flush();
    }
}

fn progress_end() {
    if io::stderr().is_terminal() {
        eprintln!();
    }
}

fn picture(path: PathBuf) -> Result<Picture> {
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
}

fn pictures(paths: Vec<PathBuf>, show_progress: bool) -> Result<Vec<Picture>> {
    if paths.is_empty() {
        return Err("输入中没有 JPEG、PNG 或 WebP 图片".into());
    }
    let total = paths.len();
    if show_progress {
        progress("校验", 0, total);
    }
    // ponytail: cap concurrent full-image decodes at 4 to bound peak memory; tune if pages are small.
    let workers = thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(total)
        .min(4);
    if workers == 1 {
        return paths
            .into_iter()
            .enumerate()
            .map(|(i, path)| {
                let result = picture(path)?;
                if show_progress {
                    progress("校验", i + 1, total);
                }
                Ok(result)
            })
            .collect();
    }
    let next = AtomicUsize::new(0);
    let (tx, rx) = mpsc::channel();
    thread::scope(|scope| {
        for _ in 0..workers {
            let tx = tx.clone();
            let paths = &paths;
            let next = &next;
            scope.spawn(move || {
                loop {
                    let i = next.fetch_add(1, AtomicOrdering::Relaxed);
                    if i >= paths.len() {
                        break;
                    }
                    let result = picture(paths[i].clone()).map_err(|error| error.to_string());
                    if tx.send((i, result)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(tx);
        let mut ordered: Vec<Option<std::result::Result<Picture, String>>> =
            (0..total).map(|_| None).collect();
        for (done, (i, result)) in rx.into_iter().enumerate() {
            ordered[i] = Some(result);
            if show_progress {
                progress("校验", done + 1, total);
            }
        }
        ordered
            .into_iter()
            .map(|result| result.unwrap().map_err(Into::into))
            .collect()
    })
}

fn add(zip: &mut ZipWriter<&mut File>, name: &str, content: &str) -> Result<()> {
    zip.start_file(
        name,
        SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
    )?;
    zip.write_all(content.as_bytes())?;
    Ok(())
}

fn package(file: &mut File, title: &str, pictures: &[Picture], rtl: bool) -> Result<()> {
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
    progress("生成", 0, pictures.len());
    for (index, picture) in pictures.iter().enumerate() {
        let n = index + 1;
        let image_name = format!("images/{n}.{}", picture.extension);
        let cover = if index == 0 {
            " properties=\"cover-image\""
        } else {
            ""
        };
        manifest.push_str(&format!(r#"<item id="image{n}" href="{image_name}" media-type="{}"{cover}/><item id="page{n}" href="pages/{n}.xhtml" media-type="application/xhtml+xml"/>"#, picture.media_type));
        spine.push_str(&format!(
            r#"<itemref idref="page{n}" properties="rendition:spread-none"/>"#
        ));
        toc.push_str(&format!(
            r#"<li><a href="pages/{n}.xhtml">第 {n} 页</a></li>"#
        ));
        // Wide/square images stay intact, centered on a portrait 2:3 page.
        let (w, h) = (
            picture.width,
            if picture.width >= picture.height {
                picture
                    .width
                    .saturating_add(picture.width / 2 + picture.width % 2)
            } else {
                picture.height
            },
        );
        add(
            &mut zip,
            &format!("EPUB/pages/{n}.xhtml"),
            &format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" lang="zh" xml:lang="zh"><head><title>第 {n} 页</title><meta name="viewport" content="width={w}, height={h}"/><style>html,body{{margin:0;padding:0;width:{w}px;height:{h}px;}}img{{display:block;width:{w}px;height:{h}px;object-fit:contain;}}</style></head><body><img src="../{image_name}" alt="第 {n} 页"/></body></html>"#
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
        progress("生成", n, pictures.len());
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
    let direction = if rtl { "rtl" } else { "ltr" };
    add(
        &mut zip,
        "EPUB/package.opf",
        &format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="book-id" prefix="rendition: http://www.idpf.org/vocab/rendition/#"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="book-id">urn:bookforge:{identifier}</dc:identifier><dc:title>{title}</dc:title><dc:language>zh</dc:language><meta property="dcterms:modified">{modified}</meta><meta property="rendition:layout">pre-paginated</meta><meta property="rendition:spread">none</meta></metadata><manifest>{manifest}</manifest><spine page-progression-direction="{direction}">{spine}</spine></package>"#
        ),
    )?;
    zip.finish()?;
    Ok(())
}

fn run() -> Result<()> {
    let args = Args::parse();
    if let Some(command) = args.command {
        return config_command(&config_path()?, command);
    }
    let directory_arg = args
        .directory
        .as_ref()
        .ok_or("请指定图片目录、ZIP 压缩包或链接")?;
    let config = Config::load(&config_path()?)?;
    let books = (args.books || config.books) && !args.no_books;
    if books && !cfg!(target_os = "macos") {
        return Err("--books 仅支持 macOS".into());
    }
    let url = directory_arg
        .to_str()
        .filter(|s| s.starts_with("http://") || s.starts_with("https://"));
    let directory = if url.is_some() {
        directory_arg.clone()
    } else {
        directory_arg.canonicalize()?
    };
    let archive = url.is_none() && directory.is_file();
    if archive
        && !directory
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
    {
        return Err("只支持 ZIP 压缩包".into());
    }
    let title = if let Some(url) = url {
        url_title(url)
    } else {
        (if archive {
            directory.file_stem()
        } else {
            directory.file_name()
        })
        .and_then(|s| s.to_str())
        .unwrap_or("book")
        .to_string()
    };
    let rtl = args.r2l || (!args.l2r && japanese_title(&title));
    let output = configured_output(&title, &args, &config)?;
    let (extracted, paths) = if let Some(url) = url {
        let (temp, paths) = download(url)?;
        (Some(temp), paths)
    } else if archive {
        let extracted = extract(&directory, !args.dry_run);
        if !args.dry_run {
            progress_end();
        }
        let (temp, paths) = extracted?;
        (Some(temp), paths)
    } else {
        (None, directory_paths(&directory)?)
    };
    let root = extracted
        .as_ref()
        .map_or(directory.as_path(), |temp| temp.0.as_path());
    let validated = pictures(paths, !args.dry_run);
    if !args.dry_run {
        progress_end();
    }
    let mut pictures = validated?;
    println!(
        "排序：{}",
        sort_pictures(&mut pictures, args.sort, root, archive)?
    );
    println!(
        "阅读方向：{}；竖屏单页",
        if rtl { "从右往左" } else { "从左往右" }
    );
    if args.dry_run {
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
                if archive {
                    format!(
                        "{}!{}",
                        directory.display(),
                        picture.path.strip_prefix(root)?.display()
                    )
                } else if url.is_some() {
                    picture.path.strip_prefix(root)?.display().to_string()
                } else {
                    picture.path.display().to_string()
                },
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
        if books {
            println!("--books：生成后将尝试用 Books 打开");
        }
        println!("仅预览，未写入文件。");
        return Ok(());
    }
    println!(
        "{} 页，封面：{}",
        pictures.len(),
        pictures[0].path.strip_prefix(root)?.display()
    );
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output)?;
    let result = package(&mut file, &title, &pictures, rtl).and_then(|_| {
        file.sync_all()?;
        Ok(())
    });
    drop(file);
    progress_end();
    if let Err(error) = result {
        if let Err(cleanup) = fs::remove_file(&output) {
            eprintln!("无法删除不完整输出 {}: {cleanup}", output.display());
        }
        return Err(error);
    }
    println!("已生成：{}（{} 页）", output.display(), pictures.len());
    #[cfg(target_os = "macos")]
    if books {
        let status = Command::new("/usr/bin/open")
            .arg("-a")
            .arg("Books")
            .arg(output.canonicalize()?)
            .status()?;
        if !status.success() {
            return Err(format!(
                "Books 打开失败（{status}）；EPUB 已保留：{}",
                output.display()
            )
            .into());
        }
        println!("已交给 Books 打开，请在 Books 中确认导入。");
    }
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
        assert_eq!(progress_bar(0, 2), "[--------------------] 0/2");
        assert_eq!(progress_bar(1, 2), "[##########----------] 1/2");
        assert_eq!(progress_bar(2, 2), "[####################] 2/2");
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
        assert!(
            Args::try_parse_from(["bookforge", "images", "--books", "--dry-run"])
                .unwrap()
                .books
        );
        assert_eq!(args.sort, Sort::Time);
        assert!(japanese_title("まんが"));
        assert!(japanese_title("ｶﾀｶﾅ.zip"));
        assert!(!japanese_title("漫画"));
        assert!(!japanese_title("Comic"));
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
    fn config_and_precedence() {
        let root = std::env::temp_dir().join(format!(
            "bookforge-config-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = root.join("bookforge/config.toml");
        assert!(Config::load(&path).unwrap().desktop.is_none());
        config_command(&path, ConfigCommand::Init).unwrap();
        assert!(config_command(&path, ConfigCommand::Init).is_err());
        config_command(
            &path,
            ConfigCommand::Config {
                action: ConfigAction::Set {
                    key: "output_dir".into(),
                    value: root.to_string_lossy().into(),
                },
            },
        )
        .unwrap();
        let config = Config::load(&path).unwrap();
        assert_eq!(config.desktop, Some(false));
        let args = Args::try_parse_from(["bookforge", "images"]).unwrap();
        assert_eq!(
            configured_output("book", &args, &config).unwrap(),
            root.join("book.epub")
        );
        let args = Args::try_parse_from(["bookforge", "images", "-o", "other"]).unwrap();
        assert_eq!(
            configured_output("book", &args, &config).unwrap(),
            Path::new("other.epub")
        );
        let args = Args::try_parse_from(["bookforge", "images", "--no-books"]).unwrap();
        assert!(args.no_books);
        config_command(
            &path,
            ConfigCommand::Config {
                action: ConfigAction::Set {
                    key: "books".into(),
                    value: "true".into(),
                },
            },
        )
        .unwrap();
        assert!(Config::load(&path).unwrap().books);
        let before = fs::read(&path).unwrap();
        assert!(
            config_command(
                &path,
                ConfigCommand::Config {
                    action: ConfigAction::Set {
                        key: "books".into(),
                        value: "maybe".into()
                    },
                }
            )
            .is_err()
        );
        assert_eq!(fs::read(&path).unwrap(), before);
        fs::write(&path, "books = perhaps").unwrap();
        assert!(Config::load(&path).is_err());
        fs::remove_dir_all(root).unwrap();
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
