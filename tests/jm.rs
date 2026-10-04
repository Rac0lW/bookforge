#![cfg(unix)]

use std::{
    fs,
    io::Read,
    os::unix::fs::PermissionsExt,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn jm_routes_downloads_and_detects_isolated_python() {
    let root = std::env::temp_dir().join(format!(
        "bookforge-jm-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let python = Command::new("python3")
        .args(["-c", "import sys; print(sys.executable)"])
        .output()
        .unwrap();
    assert!(python.status.success());
    let python = String::from_utf8(python.stdout).unwrap();
    let python = python.trim();
    let tool = root.join("jmcomic");
    fs::write(&tool, format!("#!{python}\n")).unwrap();
    fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();
    for index in 1..=3 {
        image::RgbaImage::from_pixel(2, 3, image::Rgba([index, 0, 0, 255]))
            .save(root.join(format!("{index}.png")))
            .unwrap();
    }
    fs::write(root.join("jmcomic.py"), r#"
import os
from pathlib import Path
from types import SimpleNamespace
class Photo:
    def __init__(self, album, index, count):
        self.from_album, self.index, self.count = album, index, count
        self.name = 'chapter/title'
    def __len__(self): return self.count
class Album:
    name = 'test/album'
    def __iter__(self): return iter(self.photos)
class JmOption:
    @classmethod
    def construct(cls, value):
        assert value['download']['image']['decode'] is True
        assert value['download']['threading']['photo'] == 1
        return value
def download(kind, jmid, option):
    assert jmid == '1449263'
    Path(os.environ['JM_TEST_ROOT'], 'kind').write_text(kind)
    base = Path(option['dir_rule']['base_dir'])
    base.mkdir()
    Path(os.environ['JM_TEST_ROOT'], 'temp').write_text(str(base.parent))
    album = Album()
    album.photos = [Photo(album, 1, 2), Photo(album, 2, 1)]
    selected = album.photos[:1] if kind == 'photo' else album.photos
    success = {}
    for photo in reversed(selected):
        items = []
        for index in reversed(range(1, len(photo)+1)):
            fixture = index if photo.index == 1 else 3
            path = base / f'{photo.index}-{index}.png'
            path.write_bytes(Path(os.environ['JM_TEST_ROOT'], f'{fixture}.png').read_bytes())
            items.append((str(path), SimpleNamespace(index=index)))
        success[photo] = items
    if os.environ.get('JM_TEST_INCOMPLETE'): success[selected[0]].pop()
    return (selected[0] if kind == 'photo' else album), SimpleNamespace(download_success_dict={album:success})
def download_photo(jmid, option): return download('photo', jmid, option)
def download_album(jmid, option): return download('album', jmid, option)
"#).unwrap();
    let output = root.join("book.epub");
    let invoke = |input: &str, flags: &[&str]| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_bookforge"));
        command
            .arg(input)
            .arg("-o")
            .arg(&output)
            .arg("--no-books")
            .args(flags)
            .env_remove("BOOKFORGE_JM_PYTHON")
            .env("PATH", &root)
            .env("PYTHONPATH", &root)
            .env("JM_TEST_ROOT", &root);
        command.output().unwrap()
    };
    let preview = invoke("https://18comic.vip/photo/1449263", &["--dry-run"]);
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    assert_eq!(fs::read_to_string(root.join("kind")).unwrap(), "photo");
    assert!(String::from_utf8_lossy(&preview.stdout).contains("00000001.png [封面]"));
    assert!(!output.exists());
    assert!(!std::path::Path::new(&fs::read_to_string(root.join("temp")).unwrap()).exists());
    let result = invoke("1449263", &["--delete"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(fs::read_to_string(root.join("kind")).unwrap(), "album");
    assert!(String::from_utf8_lossy(&result.stdout).contains("3 页"));
    let mut zip = zip::ZipArchive::new(fs::File::open(&output).unwrap()).unwrap();
    for index in 1..=3 {
        let mut bytes = Vec::new();
        zip.by_name(&format!("EPUB/images/{index}.png"))
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(bytes, fs::read(root.join(format!("{index}.png"))).unwrap());
    }
    drop(zip);
    fs::remove_file(&output).unwrap();
    let failure = Command::new(env!("CARGO_BIN_EXE_bookforge"))
        .args(["1449263", "--no-books", "-o"])
        .arg(&output)
        .env("BOOKFORGE_JM_PYTHON", python)
        .env("PYTHONPATH", &root)
        .env("JM_TEST_ROOT", &root)
        .env("JM_TEST_INCOMPLETE", "1")
        .output()
        .unwrap();
    assert!(!failure.status.success());
    assert!(!output.exists());
    assert!(!std::path::Path::new(&fs::read_to_string(root.join("temp")).unwrap()).exists());
    fs::remove_file(&tool).unwrap();
    let missing = invoke("1449263", &[]);
    assert!(!missing.status.success());
    let error = String::from_utf8_lossy(&missing.stderr);
    assert!(error.contains("未检测到可用的 jmcomic"));
    assert!(!error.contains("pip install") && !error.contains("brew install"));
    fs::create_dir(root.join("1449263")).unwrap();
    fs::copy(root.join("1.png"), root.join("1449263/1.png")).unwrap();
    let local = Command::new(env!("CARGO_BIN_EXE_bookforge"))
        .current_dir(&root)
        .args(["1449263", "--dry-run", "--no-books", "-o"])
        .arg(&output)
        .env("PATH", &root)
        .output()
        .unwrap();
    assert!(
        local.status.success(),
        "{}",
        String::from_utf8_lossy(&local.stderr)
    );
    fs::remove_dir_all(root).unwrap();
}
