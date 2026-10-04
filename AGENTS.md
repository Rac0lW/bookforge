## Shell 命令

优先通过 `rtk` 执行 Shell 命令；不支持的命令或需要原始输出时使用 `rtk proxy`。若 RTK 不可用，说明后使用原生命令。

## 测试约束

- 测试、冒烟验证及样书检查禁止导入 Apple Books，禁止调用 `open -a Books` 或在实际转换命令中使用 `--books`。
- 所有测试中的 EPUB 转换命令必须显式传入 `--no-books`，包括 `--dry-run`，避免用户配置 `books = true` 触发导入。
- 测试产物只保存在项目 `target/` 或临时目录；使用 EPUB 结构检查、图片检查及 EPUBCheck 验证。
- 后续如需验证 Books 导入逻辑，只能模拟调用，不能操作真实书库。
