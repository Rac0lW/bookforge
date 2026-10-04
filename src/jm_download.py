"""Bridge to the user's installed jmcomic. No dependency installation."""
import sys
from pathlib import Path

import jmcomic

kind, jm_id, destination = sys.argv[1:]
root = Path(destination)
option = jmcomic.JmOption.construct({
    "log": False,
    "dir_rule": {"base_dir": str(root / "download"), "rule": "Bd/Pindex"},
    "download": {"image": {"decode": True, "suffix": ".png"},
                 "threading": {"photo": 1, "image": 4}},
})
detail, downloader = getattr(jmcomic, "download_" + kind)(jm_id, option)
photos = [detail] if kind == "photo" else list(detail)
successful = downloader.download_success_dict
pages = []
for photo in photos:
    images = successful.get(photo.from_album, {}).get(photo, [])
    if not images or len(images) != len(photo):
        raise RuntimeError("章节图片下载不完整，停止生成 EPUB")
    for filename, image in sorted(images, key=lambda item: item[1].index):
        source = Path(filename)
        if source.suffix.lower() not in {".jpg", ".jpeg", ".png", ".webp"}:
            raise RuntimeError("不支持的 JM 图片格式：" + source.suffix)
        pages.append(source)
if not pages:
    raise RuntimeError("JM 下载没有图片")
for index, source in enumerate(pages, 1):
    source.rename(root / f"{index:08d}{source.suffix.lower()}")
(root / "title.txt").write_text(detail.name, encoding="utf-8")
