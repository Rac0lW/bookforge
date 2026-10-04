# bookforge

Turn an image folder, ZIP archive, or image URL into a fixed-layout EPUB 3 book, built with Apple Books in mind.

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

# Optional persistent defaults
bookforge init
bookforge open                           # open config with the default app
bookforge config set books true          # macOS: open new EPUBs in Books
bookforge config set output_dir ~/Books  # default destination (directory must exist)
bookforge config set desktop true        # restore desktop default
bookforge config set desktop false       # use current directory by default

# Choose an output path
bookforge ./pictures -o ./books/comic.epub
bookforge ./comic.zip -o ./books/comic.epub
bookforge ./comic.zip -o ./books/comic.epub -d # ask y/n before deleting the source ZIP
bookforge 'https://example.org/album' -o ./books/album.epub
bookforge 'https://18comic.vip/photo/1449263' -o ./books/chapter.epub
bookforge 1449263 -o ./books/album.epub # also accepts JM1449263

# macOS: generate and attempt to import into Books
bookforge ./comic.zip -o ./books/comic.epub --books

# Choose a filename on your desktop
bookforge ./pictures -D -o comic.epub

# Preview the order, cover and destination without writing a file
bookforge ./pictures --sort auto --dry-run

# Explicit natural filename or chronological ordering
bookforge ./pictures --sort name
bookforge ./pictures --sort time

# Override page direction (otherwise detected from kana in the folder/ZIP name)
bookforge ./comic.zip --r2l
bookforge ./comic.zip --l2r

bookforge --help
```

CLI help and status messages are currently in Chinese.

## Features

- **Apple Books first:** EPUB 3 fixed layout, always one portrait page per image. Portrait images retain their dimensions; wide/square images are centered intact on a portrait 2:3 page. The first image is both the cover and the first content page. Defaults to right-to-left when the folder/ZIP title contains Japanese kana; use `--r2l` or `--l2r` to override (kanji-only titles are ambiguous and default to left-to-right).
- **JPEG, PNG and static WebP:** extensions are case-insensitive. JPEG/PNG bytes are preserved; WebP is converted to PNG, preserving decoded pixels and transparency. Conversion may increase file size. Source files are never modified.
- **Explicit failures:** animated WebP and damaged images are rejected rather than silently skipped. Empty image folders also fail.
- **Safe output:** existing files are never overwritten. Failed writes attempt to remove the incomplete output.
- **Optional source deletion:** `-d` / `--delete` asks `[y/N]` after successful output. Only `y` (case-insensitive) deletes the original ZIP or the entire input directory and all its contents. Other answers, Enter, or EOF retain the source. Preview and failed output retain it too. If the EPUB is inside the source directory, that directory is retained to protect the book. URL inputs have no local source to delete. The desktop shortcut is now `-D` / `--desktop`.
- **URL input:** HTTP(S) links are downloaded with `gallery-dl` into a temporary directory, cleaned up after use. On macOS interactive terminals, if missing, asks before running `brew install gallery-dl`; elsewhere or noninteractively, install it yourself. Unsupported sites or links without usable images fail. The default EPUB name uses the last URL path segment; use `-o` for a preferred name. `--dry-run` still downloads images, but does not write an EPUB.
- **JM input:** `18comic.vip` (including `www`) `/photo/ID` links use the locally installed `jmcomic` to download one chapter; `/album/ID`, bare numeric IDs and `JMID` download the whole album. Existing local paths take precedence. Detection checks Python imports, including the isolated interpreter behind uv/pipx executables. Missing jmcomic only prompts you to download and install it yourself; no installation commands or automatic installation are offered. Set `BOOKFORGE_JM_PYTHON` to your environment's Python if needed; downloading source alone is insufficient unless that Python can import it.
- **JM ordering and output:** jmcomic decodes static images to PNG; GIF pages are unsupported and stop conversion with an error. Pages are numbered in chapter and page order. The default filename uses the downloaded title with unsafe filename characters removed; `-o` overrides it. Built-in settings use jmcomic's default client without loading `JM_OPTION_PATH` or user plugins. Failed or incomplete downloads stop conversion, and temporary images are cleaned up. `--dry-run` still downloads and validates images without writing an EPUB. JM inputs have no local source to delete with `--delete`. Use `--r2l` / `--l2r` to override reading direction.
- **ZIP input:** extracts JPEG, PNG and static WebP images (including nested folders) to a temporary directory; non-image files are ignored. Auto sorting uses natural order of full paths inside ZIP; explicit `--sort time` prefers EXIF then ZIP entry timestamps (or archive mtime if missing). Temporary files are cleaned up; limits are 512 MiB per image and 2 GiB total. Other archive formats are not supported.
- **Parallel validation:** image decoding uses up to four CPU cores (when available), preserving page order and bounding peak memory. ZIP extraction and EPUB writing remain sequential.
- **Concise output and progress:** normal runs show the sort choice, page count, cover and result; interactive terminals also show progress bars for extraction, validation and packaging. Progress is hidden when output is redirected.
- **Read-only preview:** `--dry-run` validates images and shows their full order, cover, conversion markers and output path without creating or changing the output file. Time-based sorting also shows timestamp sources and UTC timestamps.
- **macOS Books import:** `--books` or `books = true` in the config asks macOS to open the completed EPUB in Books; `--no-books` disables that default for one run. On other systems enabling Books fails before generating anything. With `--dry-run` it only previews; a launch failure leaves the EPUB intact. A successful launch does not guarantee that Books finished importing—check the Books library.

Ordinary directories are scanned without recursion. HEIC and OCR are not supported. Page alternative text currently contains only page numbers, not image descriptions.

### Configuration and output rules

The optional `~/.config/bookforge/config.toml` uses `books = false` and `desktop = true` by default. `bookforge init` creates it without overwriting an existing file; `bookforge config set books true|false`, `config set desktop true|false`, and `config set output_dir PATH` create/update it. Setting `desktop` clears `output_dir`; setting `output_dir` disables the desktop default. Paths in the file may start with `~/`, and relative paths use the current working directory. The destination directory must exist. Invalid config is reported, not ignored.

| Options | Destination |
| --- | --- |
| Neither `-o` nor `-D` | Configured default destination (desktop unless changed), named `<folder-name>.epub`, `<archive-name>.epub`, or `<last-URL-segment>.epub` |
| `-o filename` | Current working directory |
| `-o path/to/filename` | The specified path |
| `-D` | System desktop, named `<folder-name>.epub` |
| `-D -o filename` | System desktop, with a custom filename |

`-o` / `--output` overrides the configured destination and appends `.epub` when no extension is supplied. `-D` forces the desktop; with `-D` / `--desktop`, `-o` must be a filename, not a directory path. The output parent directory must already exist. If the system desktop cannot be located, use an explicit `-o` path.

### Sorting

- **`auto` (default):** use natural filename ordering when all filename stems contain a single numeric run with the same prefix/suffix and unique numeric values. Otherwise, try chronological ordering. If timestamps are duplicated, fall back to natural filename ordering and report the ambiguity. A single image uses name ordering.
- **`name`:** case-sensitive natural filename ordering (`1`, `2`, `10`). Equal numeric values are resolved by the full filename.
- **`time`:** oldest first, preferring EXIF `DateTimeOriginal`, including timezone offsets and subseconds. Missing or invalid capture times fall back to file modification times. Ties use natural filename ordering.

Automatic ordering is a heuristic, not a guarantee. For filenames with chapter and page numbers, explicitly use `--sort name`. EXIF timestamps without timezone offsets are interpreted in the current machine's local timezone; ambiguous or nonexistent daylight-saving times fall back to modification times. Images from different timezones, or copied/edited files, may need manual review with `--dry-run`.

## Development and validation

Tests must never import samples into Books. Every test conversion must explicitly pass `--no-books` so user defaults cannot trigger an import. Validate local artifacts with structure checks and EPUBCheck.

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
python3 scripts/smoke.py
```

The smoke check creates a portrait/landscape/square sample and checks EPUB structure, sorting, EXIF timezones and fallbacks, cover selection, original bytes, output protection, desktop paths and read-only previews. Desktop checks do not write to your actual desktop.

```sh
# Print the temporary WebP sample path
cargo test --test webp -- --nocapture
```

WebP tests check dimensions, decoded pixels, transparency, mixed-format ordering, cover metadata, source preservation, and rejection of animated or damaged files.

Generated samples have passed [EPUBCheck 5.3.0](https://github.com/w3c/epubcheck/releases) without errors or warnings. EPUBCheck is an optional validation tool, not a build dependency. Download and extract it before running:

```sh
java -jar /path/to/epubcheck.jar target/smoke/apple-books-sample.epub
```

The single-page sample was reported to display correctly in Apple Books. Wide/square images on portrait pages, WebP sample rendering and iPhone/iPad zoom still need manual verification. The smoke sample should show a red portrait, green landscape and blue square, with all four black borders visible and the red page as its cover.

## References

- [Apple Books Asset Guide](https://help.apple.com/itc/booksassetguide/en.lproj/static.html)
- [EPUB 3.3](https://www.w3.org/TR/epub-33/)
- [EPUBCheck](https://github.com/w3c/epubcheck/releases)

## License

[Apache License 2.0](LICENSE).
