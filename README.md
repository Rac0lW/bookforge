# bookforge

Turn a folder or ZIP archive of images into a fixed-layout EPUB 3 book, built with Apple Books in mind.

[简体中文](README.zh-CN.md)

## Install

Install a recent stable [Rust toolchain](https://rustup.rs/), then:

```sh
git clone https://github.com/Rac0lW/bookforge.git
cd bookforge
cargo install --path .
```

## Usage

```sh
# Save to your desktop as pictures.epub
bookforge ./pictures

# Choose an output path
bookforge ./pictures -o ./books/comic.epub
bookforge ./comic.zip -o ./books/comic.epub

# macOS: generate and attempt to import into Books
bookforge ./comic.zip -o ./books/comic.epub --books

# Choose a filename on your desktop
bookforge ./pictures -d -o comic.epub

# Preview the order, cover and destination without writing a file
bookforge ./pictures --sort auto --dry-run

# Explicit natural filename or chronological ordering
bookforge ./pictures --sort name
bookforge ./pictures --sort time

bookforge --help
```

CLI help and status messages are currently in Chinese.

## Features

- **Apple Books first:** EPUB 3 fixed layout, one image per page, using the image's dimensions and a single-page spread. The first image is both the cover and the first content page.
- **JPEG, PNG and static WebP:** extensions are case-insensitive. JPEG/PNG bytes are preserved; WebP is converted to PNG, preserving decoded pixels and transparency. Conversion may increase file size. Source files are never modified.
- **Explicit failures:** animated WebP and damaged images are rejected rather than silently skipped. Empty image folders also fail.
- **Safe output:** existing files are never overwritten. Failed writes attempt to remove the incomplete output.
- **ZIP input:** extracts JPEG, PNG and static WebP images (including nested folders) to a temporary directory; non-image files are ignored. Auto sorting uses natural order of full paths inside ZIP; explicit `--sort time` prefers EXIF then ZIP entry timestamps (or archive mtime if missing). Temporary files are cleaned up; limits are 512 MiB per image and 2 GiB total. Other archive formats are not supported.
- **Parallel validation:** image decoding uses up to four CPU cores (when available), preserving page order and bounding peak memory. ZIP extraction and EPUB writing remain sequential.
- **Concise output and progress:** normal runs show the sort choice, page count, cover and result; interactive terminals also show progress bars for extraction, validation and packaging. Progress is hidden when output is redirected.
- **Read-only preview:** `--dry-run` validates images and shows their full order, cover, conversion markers and output path without creating or changing the output file. Time-based sorting also shows timestamp sources and UTC timestamps.
- **macOS Books import:** `--books` asks macOS to open the completed EPUB in Books. On other systems the option fails before generating anything. With `--dry-run` it only previews; a launch failure leaves the EPUB intact. A successful launch does not guarantee that Books finished importing—check the Books library.

Ordinary directories are scanned without recursion. HEIC and OCR are not supported. Page alternative text currently contains only page numbers, not image descriptions.

### Output rules

| Options | Destination |
| --- | --- |
| Neither `-o` nor `-d` | System desktop, named `<folder-name>.epub` or `<archive-name>.epub` |
| `-o filename` | Current working directory |
| `-o path/to/filename` | The specified path |
| `-d` | System desktop, named `<folder-name>.epub` |
| `-d -o filename` | System desktop, with a custom filename |

`-o` / `--output` appends `.epub` when no extension is supplied. With `-d` / `--desktop`, `-o` must be a filename, not a directory path. The output parent directory must already exist. If the system desktop cannot be located, use an explicit `-o` path.

### Sorting

- **`auto` (default):** use natural filename ordering when all filename stems contain a single numeric run with the same prefix/suffix and unique numeric values. Otherwise, try chronological ordering. If timestamps are duplicated, fall back to natural filename ordering and report the ambiguity. A single image uses name ordering.
- **`name`:** case-sensitive natural filename ordering (`1`, `2`, `10`). Equal numeric values are resolved by the full filename.
- **`time`:** oldest first, preferring EXIF `DateTimeOriginal`, including timezone offsets and subseconds. Missing or invalid capture times fall back to file modification times. Ties use natural filename ordering.

Automatic ordering is a heuristic, not a guarantee. For filenames with chapter and page numbers, explicitly use `--sort name`. EXIF timestamps without timezone offsets are interpreted in the current machine's local timezone; ambiguous or nonexistent daylight-saving times fall back to modification times. Images from different timezones, or copied/edited files, may need manual review with `--dry-run`.

## Development and validation

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
python3 scripts/smoke.py
```

The smoke check creates a portrait/landscape/square sample and checks EPUB structure, sorting, EXIF timezones and fallbacks, cover selection, original bytes, output protection, desktop paths and read-only previews. Desktop checks do not write to your actual desktop.

```sh
# macOS: inspect the sample in Apple Books
open -a Books target/smoke/apple-books-sample.epub

# Print the temporary WebP sample path
cargo test --test webp -- --nocapture
```

WebP tests check dimensions, decoded pixels, transparency, mixed-format ordering, cover metadata, source preservation, and rejection of animated or damaged files.

Generated samples have passed [EPUBCheck 5.3.0](https://github.com/w3c/epubcheck/releases) without errors or warnings. EPUBCheck is an optional validation tool, not a build dependency. Download and extract it before running:

```sh
java -jar /path/to/epubcheck.jar target/smoke/apple-books-sample.epub
```

The initial sample was reported to display correctly in Apple Books. WebP sample rendering and iPhone/iPad rotation and zoom still need separate manual verification. The smoke sample should show a red portrait, green landscape and blue square, with all four black borders visible and the red page as its cover.

## References

- [Apple Books Asset Guide](https://help.apple.com/itc/booksassetguide/en.lproj/static.html)
- [EPUB 3.3](https://www.w3.org/TR/epub-33/)
- [EPUBCheck](https://github.com/w3c/epubcheck/releases)

## License

[Apache License 2.0](LICENSE).
