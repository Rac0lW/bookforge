"""Run with python3 scripts/smoke.py; keeps the Apple Books sample in target/smoke/."""
import binascii
import os
import pathlib
import struct
import subprocess
import xml.etree.ElementTree as ET
import zipfile
import zlib

ROOT = pathlib.Path(__file__).resolve().parents[1]
WORK = ROOT / "target" / "smoke"
IMAGES = WORK / "测试 & 图片"
IMAGES.mkdir(parents=True, exist_ok=True)


def png(width, height, color):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", binascii.crc32(kind + data) & 0xffffffff)
    # Contrasting frame makes cropping visible. Each page has a different color/shape.
    rows = []
    for y in range(height):
        rows.append(b"\0" + b"".join(bytes((0, 0, 0) if min(x, y, width - x - 1, height - y - 1) < 12 else color)
                                   for x in range(width)))
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(b"".join(rows))) + chunk(b"IEND", b""))


for name, w, h, color in [("1.png", 600, 900, (220, 70, 70)),
                           ("2.png", 900, 600, (60, 170, 90)),
                           ("10.png", 700, 700, (60, 100, 220))]:
    (IMAGES / name).write_bytes(png(w, h, color))
subprocess.run(["cargo", "build"], cwd=ROOT, check=True)
BINARY = ROOT / "target" / "debug" / "bookforge"
OUTPUT = WORK / "apple-books-sample.epub"
OUTPUT.unlink(missing_ok=True)
command = [str(BINARY), str(IMAGES), "-o", str(OUTPUT)]
subprocess.run(command, check=True)
with zipfile.ZipFile(OUTPUT) as epub:
    first = epub.infolist()[0]
    assert first.filename == "mimetype" and first.compress_type == zipfile.ZIP_STORED
    assert epub.read("mimetype") == b"application/epub+zip"
    for name in epub.namelist():
        if name.endswith((".xml", ".opf", ".xhtml")):
            ET.fromstring(epub.read(name))  # noqa: S314 - generated locally by this test
    ns = {"opf": "http://www.idpf.org/2007/opf"}
    package = ET.fromstring(epub.read("EPUB/package.opf"))  # noqa: S314 - generated locally by this test
    assert [item.attrib["idref"] for item in package.findall("opf:spine/opf:itemref", ns)] == ["page1", "page2", "page3"]
    cover = package.find("opf:manifest/opf:item[@properties='cover-image']", ns)
    assert cover is not None and cover.attrib["href"] == "images/1.png"
    for index, name in enumerate(["1.png", "2.png", "10.png"], 1):
        assert epub.read(f"EPUB/images/{index}.png") == (IMAGES / name).read_bytes()
original = OUTPUT.read_bytes()
assert subprocess.run(command, capture_output=True).returncode != 0
assert OUTPUT.read_bytes() == original
bad = WORK / "bad"
bad.mkdir(exist_ok=True)
(bad / "1.png").write_bytes(b"damaged PNG")
failed_output = WORK / "bad.epub"
failed_output.unlink(missing_ok=True)
assert subprocess.run([str(BINARY), str(bad), "-o", str(failed_output)], capture_output=True).returncode != 0
assert not failed_output.exists()


def preview(directory, *options):
    result = subprocess.run([str(BINARY), str(directory), "--dry-run", *options],
                            capture_output=True, text=True, check=True)
    order = [pathlib.Path(line.split(": ", 1)[1].split(" [", 1)[0]).name
             for line in result.stdout.splitlines() if line.split(":", 1)[0].isdigit()]
    return result.stdout, order


# Numeric sequence takes priority over conflicting modification times.
for index, name in enumerate(["10.png", "2.png", "1.png"]):
    os.utime(IMAGES / name, (1700000000 + index, 1700000000 + index))
preview_output = WORK / "dry-run-must-not-exist.epub"
preview_output.unlink(missing_ok=True)
text, order = preview(IMAGES, "-o", str(preview_output))
assert "auto → name" in text and order == ["1.png", "2.png", "10.png"]
assert not preview_output.exists()
assert preview(IMAGES, "--sort", "time")[1] == ["10.png", "2.png", "1.png"]
assert preview(IMAGES, "--sort", "name")[1] == ["1.png", "2.png", "10.png"]
# Preview never truncates an existing output.
preview(IMAGES, "-o", str(OUTPUT))
assert OUTPUT.read_bytes() == original

photos = WORK / "photos"
photos.mkdir(exist_ok=True)
for index, name in enumerate(["c.png", "a.png", "b.png"]):
    (photos / name).write_bytes(png(40, 48, (80 + index * 40, 100, 120)))
    os.utime(photos / name, (1700000000 + index, 1700000000 + index))
text, order = preview(photos)
assert "auto → time" in text and order == ["c.png", "a.png", "b.png"]
time_output = WORK / "time.epub"
time_output.unlink(missing_ok=True)
subprocess.run([str(BINARY), str(photos), "--sort", "time", "-o", str(time_output)], check=True)
with zipfile.ZipFile(time_output) as epub:
    for index, name in enumerate(["c.png", "a.png", "b.png"], 1):
        assert epub.read(f"EPUB/images/{index}.png") == (photos / name).read_bytes()
for path in photos.glob("*.png"):
    os.utime(path, (1700000000, 1700000000))
text, order = preview(photos)
assert "auto → name" in text and "相同时间" in text and order == ["a.png", "b.png", "c.png"]
assert preview(photos, "--sort", "time")[1] == ["a.png", "b.png", "c.png"]


def with_exif(data, date, offset):
    # Minimal valid TIFF: IFD0 -> ExifIFD with DateTimeOriginal/OffsetTimeOriginal.
    values = [(0x9003, date.encode() + b"\0"), (0x9011, offset.encode() + b"\0")]
    tiff = b"II\x2a\0" + struct.pack("<I", 8)
    tiff += struct.pack("<H", 1) + struct.pack("<HHII", 0x8769, 4, 1, 26) + struct.pack("<I", 0)
    value_offset = 26 + 2 + 12 * len(values) + 4
    entries, payload = b"", b""
    for tag, value in values:
        entries += struct.pack("<HHII", tag, 2, len(value), value_offset + len(payload))
        payload += value
    tiff += struct.pack("<H", len(values)) + entries + struct.pack("<I", 0) + payload
    body = b"eXIf" + tiff
    chunk = struct.pack(">I", len(tiff)) + body + struct.pack(">I", binascii.crc32(body) & 0xffffffff)
    return data[:33] + chunk + data[33:]


# EXIF wins over mtime; timezone offsets are respected; missing/invalid EXIF uses mtime.
(photos / "a.png").write_bytes(with_exif(png(24, 32, (200, 0, 0)), "2023:11:15 08:00:00", "+08:00"))
(photos / "b.png").write_bytes(with_exif(png(24, 32, (0, 200, 0)), "2023:11:15 01:00:00", "+00:00"))
os.utime(photos / "a.png", (1800000001, 1800000001))
os.utime(photos / "b.png", (1800000000, 1800000000))
text, order = preview(photos, "--sort", "time")
assert "EXIF 2 张，修改时间 1 张" in text and order == ["c.png", "a.png", "b.png"]
(photos / "b.png").write_bytes(with_exif(png(24, 32, (0, 200, 0)), "2023:99:99 01:00:00", "+00:00"))
os.utime(photos / "b.png", (1600000000, 1600000000))
text, order = preview(photos, "--sort", "time")
assert "EXIF 1 张，修改时间 2 张" in text and order == ["b.png", "c.png", "a.png"]

# Desktop paths are previewed only: never write to the real desktop in tests.
text, _ = preview(IMAGES, "--desktop", "--output", "bookforge-smoke")
assert "bookforge-smoke.epub" in text
desktop_preview = preview(IMAGES, "-d")[0]
assert "测试 & 图片.epub" in desktop_preview
assert preview(IMAGES)[0] == desktop_preview
assert "输出：custom.epub\n" in preview(IMAGES, "-o", "custom.epub")[0]
assert subprocess.run([str(BINARY), str(IMAGES), "-d", "-o", "../bad.epub", "--dry-run"], capture_output=True).returncode != 0
assert subprocess.run([str(BINARY), str(IMAGES), "--sort", "bad"], capture_output=True).returncode != 0
print(f"PASS: EPUB structure, cover, sorting modes, EXIF/timezones, dry-run, desktop paths, error handling\nSample: {OUTPUT}")
