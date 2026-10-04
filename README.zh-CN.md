# bookforge

[English](README.md)

图片文件夹、ZIP 压缩包或图片链接 → Apple Books 优先的 EPUB 3 固定版式原型。

## 安装

安装较新的稳定版 [Rust](https://rustup.rs/)，然后运行：

```sh
git clone https://github.com/Rac0lW/bookforge.git
cd bookforge
cargo install --path .
```

安装后可直接使用 `bookforge ./图片目录`；以下 `cargo run --` 示例在源码目录中执行。

## 使用

```sh
cargo run -- ./图片目录 # 默认输出到桌面：图片目录.epub
cargo run -- init # 创建 ~/.config/bookforge/config.toml，已存在则提醒且不覆盖
cargo run -- open # 使用系统默认应用打开配置文件（须先创建）
cargo run -- config set books true # macOS：以后默认打开 Books
cargo run -- config set output_dir ~/Books # 默认输出到该目录（须事先存在）
cargo run -- config set desktop true # 恢复默认输出到桌面
cargo run -- config set desktop false # 改为默认输出到当前目录
cargo run -- ./图片目录 -o ./画集.epub
cargo run -- ./图片目录 -D -o 画集.epub
cargo run -- ./图片目录 --sort auto --dry-run
cargo run -- ./图片目录 --sort time -o ./按时间.epub
cargo run -- ./漫画.zip -o ./漫画.epub
cargo run -- ./漫画.zip -o ./漫画.epub -d # 输出成功后询问 y/n，确认后删除原 ZIP
cargo run -- ./漫画.zip -o ./漫画.epub --books # macOS：生成后尝试导入 Books
cargo run -- ./漫画.zip --dry-run
cargo run -- 'https://example.org/album' -o ./画集.epub # 从链接下载后制作
cargo run -- 'https://18comic.vip/photo/1449263' -o ./章节.epub # jmcomic：下载该章节
cargo run -- 1449263 -o ./整本.epub # jmcomic：下载整本（也接受 JM1449263）
cargo run -- ./漫画.zip --r2l -o ./日漫.epub # 强制从右往左
cargo run -- ./漫画.zip --l2r -o ./左翻.epub # 强制从左往右
cargo test
python3 scripts/smoke.py
```

- HTTP(S) 链接使用 `gallery-dl` 下载到临时目录并在结束时清理；不支持的网站或没有可用图片时会报错。若未安装，macOS 交互终端会先询问是否运行 `brew install gallery-dl`；非交互环境或其他系统请自行安装。链接默认以 URL 最后一路径段命名，建议用 `-o` 指定书名；`--dry-run` 仍会下载图片，但不会创建 EPUB。
- `18comic.vip`（含 `www`）的 `/photo/序号` 链接使用本地 `jmcomic` 下载该章节；`/album/序号`、纯数字或 `JM序号` 下载整本。已存在的同名本地路径优先处理。运行时检测 Python 能否导入 jmcomic，也识别 uv／pipx 命令指向的独立 Python；找不到时只提示自行下载并安装，不提供安装命令或自动安装。特殊环境可设置 `BOOKFORGE_JM_PYTHON` 为已安装 jmcomic 的 Python 路径。仅下载源码目录还不够，所选 Python 必须能够导入它。
- JM 静态图片由 jmcomic 解码后保存为 PNG；不支持 GIF 页，遇到时停止并报错。整本按章节及页码连续编号；默认书名来自下载结果（移除文件名不允许的字符），可用 `-o` 覆盖。使用内置下载设置，不读取 `JM_OPTION_PATH` 或执行用户配置插件；网络访问使用 jmcomic 默认客户端。下载失败或图片不完整时停止，临时图片自动清理。`--dry-run` 同样下载、校验并清理图片，只有 EPUB 不写入；`--delete` 对 JM 输入没有本地原文件可删除。可用 `--r2l`／`--l2r` 指定翻页方向。
- 支持图片目录或 ZIP（含子目录）中的 JPEG／PNG／静态 WebP（扩展名不区分大小写）；普通目录仍不递归。ZIP 中非图片文件会忽略，图片按内部路径自然排序；默认 `--sort auto` 对 ZIP 使用名称排序，`--sort time` 可按 EXIF／ZIP 条目时间排序（缺失时用压缩包修改时间）。解压到临时目录并在结束时清理；每张图片限 512 MiB，总计限 2 GiB。暂不支持其他压缩格式。
- 始终竖屏单页、一图一页：竖图保留原尺寸，横图／方图完整居中于竖向 2:3 页面，不裁切原图；第一张图同时作为书架封面和正文第一页。默认从左往右翻页；文件夹名或 ZIP 名含日文假名时默认从右往左，`--r2l`／`--l2r` 可覆盖。纯汉字无法可靠区分中文与日文，请手动指定；可在阅读器里放大查看细字。JPEG／PNG 保留原始字节；WebP 自动转成 PNG 嵌入 EPUB，保留解码后的像素与透明度，不修改源文件，但输出体积可能增大。
- 动态 WebP 明确报错，不静默取第一帧；预览会标记 `[WebP → PNG]`，同样检查动态和损坏文件。
- 配置文件固定在 `~/.config/bookforge/config.toml`，无需初始化也可直接运行或使用 `config set`。`init` 创建包含 `books = false`、`desktop = true` 的默认配置，重复执行不会覆盖已有文件。可手工编辑 TOML，或使用 `config set books true|false`、`config set desktop true|false`、`config set output_dir 路径`；设置 `desktop` 会清除 `output_dir`，设置 `output_dir` 会关闭桌面默认值。目录须已存在；配置中 `~/` 展开为用户主目录，相对目录基于当前工作目录。配置出错会提示，不会静默忽略。
- 未指定 `-o` 或 `-D` 时，默认输出到系统桌面（可用配置改为指定目录或当前目录），名称为 `输入文件夹名称.epub`、`压缩包名称.epub` 或 `链接末段.epub`；`-o`／`--output` 优先于配置的默认输出位置，指定文件名或路径（相对路径基于当前目录），没有扩展名时补 `.epub`，不覆盖已有文件。
- `-D`／`--desktop` 使用系统桌面目录；与 `-o` 同用时只接受文件名，不接受目录路径。输出父目录必须已存在。
- `-d`／`--delete` 在输出成功后询问 `[y/N]`，只有输入 `y`（不区分大小写）才删除原 ZIP 或整个输入目录及其全部内容；输入 `n`、回车或输入结束均保留。预览或输出失败时不删除。EPUB 位于原目录内时保留目录，避免删除成品；链接输入没有本地原文件可删除。桌面输出的短参数现为 `-D`。
- macOS 上 `--books` 或配置 `books = true` 在 EPUB 生成成功后调用系统 `open -a Books` 尝试导入；`--no-books` 可单次关闭配置的默认导入。其他系统启用 Books 会报错。`--dry-run` 只预览，不打开 Books。打开失败时 EPUB 仍保留；命令成功只代表已交给 Books，是否导入成功请在书库中确认。
- 图片解码校验最多使用 4 个 CPU 核心并行处理（受机器可用核心数限制），减少多图输入的等待；ZIP 解压与 EPUB 写入仍顺序执行。正常运行只显示排序、页数、封面与生成结果；交互式终端在解压、校验和生成时显示进度条（重定向输出时不显示）。`--dry-run` 仍显示完整顺序、首图封面、输出路径，不创建或修改输出文件；时间排序还显示各图的时间来源和 UTC 时间。
- 空目录、损坏图片或写入错误会导致失败；写入失败尝试删除不完整输出。
- 暂不支持 HEIC、递归目录或图片文字识别。页面替代文本目前只有页码，不能替代图片内容描述。

## 排序规则

- `--sort name`：文件名自然排序（1、2、10），大小写敏感；相等数字按完整文件名确定顺序。
- `--sort time`：从早到晚，优先 EXIF `DateTimeOriginal`（支持时区偏移和亚秒）；缺失或无效时使用文件修改时间。同时间按自然名称排序。
- `--sort auto`（默认）：文件名去扩展名后，每个名称只有一段数字、前后缀一致且序号不重复时，使用自然名称排序；否则尝试时间排序。若有重复时间则退回名称排序并提示。单张图直接使用名称排序。

自动排序只是启发式，不保证阅读顺序正确；章节／页码等多段数字名称建议显式 `--sort name`。无时区 EXIF 按运行机器的本地时区解释；遇到无法确定的夏令时时刻退回修改时间。图片来自不同时区或修改时间受复制影响时，请先用 `--dry-run` 检查，再指定排序方式。

## 验证

测试禁止导入 Books；所有测试转换命令必须显式带 `--no-books`，避免用户配置触发导入。样书仅保存在本地，用结构检查和 EPUBCheck 验证。

`python3 scripts/smoke.py` 生成三张有黑色边框的测试图片，并检查 EPUB 结构、三种排序、EXIF 时区与修改时间回退、封面、原图保留、只读预览、桌面路径冲突、不覆盖文件和损坏图片报错。桌面输出仅预览，不向真实桌面写测试文件。

样书：`target/smoke/apple-books-sample.epub`。

`cargo test` 还会验证 WebP 转换后的尺寸、像素和透明度、混合格式排序与封面、源文件不变、动态／损坏 WebP 拒绝处理，以及预览不写文件。`cargo test --test webp -- --nocapture` 会打印临时 WebP 样书路径，方便额外检查；WebP 样书在 Apple Books 中的显示仍待人工确认。

已通过 Rust 测试、Clippy 和 EPUBCheck 5.3.0（0 错误、0 警告）。重新生成后可运行：

```sh
java -jar target/epubcheck/epubcheck-5.3.0/epubcheck.jar target/smoke/apple-books-sample.epub
```

EPUBCheck 不属于构建依赖；若本地不存在，请从官方发布页下载。

单页样书已反馈在 Apple Books 中显示正常；横图／方图在竖向页面上的显示及 iPhone／iPad 缩放仍待人工确认。样书应为红色竖图 → 绿色横图 → 蓝色方图，四边黑框完整，封面为红色竖图。

参考：[Apple Books Asset Guide](https://help.apple.com/itc/booksassetguide/en.lproj/static.html)、[EPUB 3.3](https://www.w3.org/TR/epub-33/)、[EPUBCheck](https://github.com/w3c/epubcheck/releases)。

## 许可证

[Apache License 2.0](LICENSE)。
