use crate::{Result, directory_paths};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub struct Source {
    pub id: String,
    photo: bool,
}

impl Source {
    pub fn parse(input: &str) -> Option<Self> {
        let (id, photo) = if let Some(url) = input
            .strip_prefix("https://")
            .or_else(|| input.strip_prefix("http://"))
        {
            let (host, path) = url.split_once('/')?;
            if !host.eq_ignore_ascii_case("18comic.vip")
                && !host.eq_ignore_ascii_case("www.18comic.vip")
            {
                return None;
            }
            let path = path.split(['?', '#']).next()?.trim_end_matches('/');
            if let Some(id) = path.strip_prefix("photo/") {
                (id, true)
            } else {
                (path.strip_prefix("album/")?, false)
            }
        } else {
            (
                input
                    .strip_prefix("JM")
                    .or_else(|| input.strip_prefix("jm"))
                    .unwrap_or(input),
                false,
            )
        };
        if id.is_empty() || !id.bytes().all(|c| c.is_ascii_digit()) || id.bytes().all(|c| c == b'0')
        {
            return None;
        }
        Some(Self {
            id: id.trim_start_matches('0').into(),
            photo,
        })
    }

    pub fn download(&self, python: &Path, root: &Path) -> Result<(String, Vec<PathBuf>)> {
        eprintln!(
            "使用 jmcomic 下载 {} {}",
            if self.photo { "章节" } else { "整本" },
            self.id
        );
        let status = Command::new(python)
            .arg("-c")
            .arg(include_str!("jm_download.py"))
            .arg(if self.photo { "photo" } else { "album" })
            .arg(&self.id)
            .arg(root)
            .status()?;
        if !status.success() {
            return Err(format!(
                "jmcomic 下载失败（{status}）；请检查序号、网络和本地 jmcomic 环境"
            )
            .into());
        }
        let name = fs::read_to_string(root.join("title.txt"))?;
        let name: String = name
            .chars()
            .filter(|c| {
                !c.is_control()
                    && !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
            })
            .collect();
        let title = name.trim().trim_matches('.');
        let title = if title.is_empty() {
            format!("JM{}", self.id)
        } else {
            title.into()
        };
        Ok((title, directory_paths(root)?))
    }
}

// uv/pipx executables point at an isolated Python; probing only python3 misses them.
pub fn python() -> Result<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(python) = std::env::var_os("BOOKFORGE_JM_PYTHON") {
        candidates.push(PathBuf::from(python));
    } else {
        if let Some(path) = std::env::var_os("PATH") {
            for directory in std::env::split_paths(&path) {
                let executable = directory.join("jmcomic");
                if let Ok(text) = fs::read_to_string(&executable)
                    && let Some(line) = text.lines().next().and_then(|line| line.strip_prefix("#!"))
                {
                    let interpreter = line.trim();
                    if Path::new(interpreter).is_absolute()
                        && !interpreter.contains(char::is_whitespace)
                    {
                        candidates.push(PathBuf::from(interpreter));
                    }
                }
            }
        }
        candidates.extend([PathBuf::from("python3"), PathBuf::from("python")]);
    }
    for candidate in candidates {
        match Command::new(&candidate)
            .args(["-c", "import jmcomic"])
            .output()
        {
            Ok(output) if output.status.success() => return Ok(candidate),
            Ok(output) => eprintln!(
                "{} 无法导入 jmcomic：{}",
                candidate.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => eprintln!("无法检测 {}：{error}", candidate.display()),
        }
    }
    Err("未检测到可用的 jmcomic；请自行下载并安装到本地 Python 环境后重试。已安装时可设置 BOOKFORGE_JM_PYTHON 为该环境的 Python 路径。".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognize_jm_inputs() {
        for value in [
            "1449263",
            "JM1449263",
            "jm01449263",
            "https://18comic.vip/album/1449263/?x=1",
        ] {
            let source = Source::parse(value).unwrap();
            assert_eq!(source.id, "1449263");
            assert!(!source.photo);
        }
        assert!(
            Source::parse("https://18comic.vip/photo/1449263#page")
                .unwrap()
                .photo
        );
        for value in [
            "0",
            "",
            "123/a",
            "https://18comic.vip.evil/photo/123",
            "https://evil/album/123",
            "https://18comic.vip/photo/123/456",
            "./123",
        ] {
            assert!(Source::parse(value).is_none(), "{value}");
        }
    }
}
