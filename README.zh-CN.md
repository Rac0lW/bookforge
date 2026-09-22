# bookforge

[English](README.md)

图片文件夹 → Apple Books 优先的 EPUB 3 固定版式原型。

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
cargo run -- ./图片目录 -o ./画集.epub
cargo run -- ./图片目录 -d -o 画集.epub
cargo run -- ./图片目录 --sort auto --dry-run
cargo run -- ./图片目录 --sort time -o ./按时间.epub
cargo test
python3 scripts/smoke.py
open -a Books target/smoke/apple-books-sample.epub
```

- 只处理指定目录的 JPEG／PNG／静态 WebP（扩展名不区分大小写），不递归；默认 `--sort auto`，会显示选择的排序规则。
- 一图一页，页面尺寸取原图尺寸；第一张图同时作为书架封面和正文第一页。JPEG／PNG 保留原始字节；WebP 自动转成 PNG 嵌入 EPUB，保留解码后的像素与透明度，不修改源文件，但输出体积可能增大。
- 动态 WebP 明确报错，不静默取第一帧；预览会标记 `[WebP → PNG]`，同样检查动态和损坏文件。
- 未指定 `-o` 或 `-d` 时，默认输出到系统桌面，名称为 `输入文件夹名称.epub`；`-o`／`--output` 指定输出文件名或路径（相对路径基于当前目录），没有扩展名时补 `.epub`，不覆盖已有文件。
- `-d`／`--desktop` 使用系统桌面目录；与 `-o` 同用时只接受文件名，不接受目录路径。输出父目录必须已存在。
- `--dry-run` 校验图片并显示完整顺序、首图封面、输出路径，不创建或修改输出文件；时间排序还显示各图的时间来源和 UTC 时间。
- 空目录、损坏图片或写入错误会导致失败；写入失败尝试删除不完整输出。
- 暂不支持 HEIC、递归目录或图片文字识别。页面替代文本目前只有页码，不能替代图片内容描述。

## 排序规则

- `--sort name`：文件名自然排序（1、2、10），大小写敏感；相等数字按完整文件名确定顺序。
- `--sort time`：从早到晚，优先 EXIF `DateTimeOriginal`（支持时区偏移和亚秒）；缺失或无效时使用文件修改时间。同时间按自然名称排序。
- `--sort auto`（默认）：文件名去扩展名后，每个名称只有一段数字、前后缀一致且序号不重复时，使用自然名称排序；否则尝试时间排序。若有重复时间则退回名称排序并提示。单张图直接使用名称排序。

自动排序只是启发式，不保证阅读顺序正确；章节／页码等多段数字名称建议显式 `--sort name`。无时区 EXIF 按运行机器的本地时区解释；遇到无法确定的夏令时时刻退回修改时间。图片来自不同时区或修改时间受复制影响时，请先用 `--dry-run` 检查，再指定排序方式。

## 验证

`python3 scripts/smoke.py` 生成三张有黑色边框的测试图片，并检查 EPUB 结构、三种排序、EXIF 时区与修改时间回退、封面、原图保留、只读预览、桌面路径冲突、不覆盖文件和损坏图片报错。桌面输出仅预览，不向真实桌面写测试文件。

样书：`target/smoke/apple-books-sample.epub`。

`cargo test` 还会验证 WebP 转换后的尺寸、像素和透明度、混合格式排序与封面、源文件不变、动态／损坏 WebP 拒绝处理，以及预览不写文件。`cargo test --test webp -- --nocapture` 会打印临时 WebP 样书路径，方便额外检查；WebP 样书在 Apple Books 中的显示仍待人工确认。

已通过 Rust 测试、Clippy 和 EPUBCheck 5.3.0（0 错误、0 警告）。重新生成后可运行：

```sh
java -jar target/epubcheck/epubcheck-5.3.0/epubcheck.jar target/smoke/apple-books-sample.epub
```

EPUBCheck 不属于构建依赖；若本地不存在，请从官方发布页下载。

用户已反馈首版样书在 Apple Books 中看起来正常。iPhone／iPad 的横竖屏切换和缩放尚未单独确认；重新检查时，样书应为红色竖图 → 绿色横图 → 蓝色方图，四边黑框完整，封面为红色竖图。

参考：[Apple Books Asset Guide](https://help.apple.com/itc/booksassetguide/en.lproj/static.html)、[EPUB 3.3](https://www.w3.org/TR/epub-33/)、[EPUBCheck](https://github.com/w3c/epubcheck/releases)。

## 许可证

[Apache License 2.0](LICENSE)。
