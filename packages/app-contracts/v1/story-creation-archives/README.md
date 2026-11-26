# 只读创作归档 schema

本目录每个 `<sha256>.json` 是某一时期四份创作 schema（`story-creation`、`-model`、`-content`、`clip-workflow`）经 `bgx-contract-gen::story_creation::merged_definitions` 相同规则合并、规范化 `$ref` 并按键排序后的精确 UTF-8 定义图。文件字节 SHA256 等于文件名，不添加换行或更改排版；读取时再次核摘要。登记入口是 `src-tauri/src/story_creation/archive_transport/schema.rs` 的 `FROZEN_GRAPHS`。

| 摘要 | 来源提交 | 说明 |
|---|---|---|
| `6002f32f…` | `340127743a661d51768ba42f419e49894b990965` | 与 9f75 只在 IntegrationReply 定义不同 |
| `9f75c1d7…` | `cd1f4c15cf06b7577f5c2f8f7ea558cb870d26bb` | 2026-10-27 至 2026-11-10 的现行图；`6467f3419` 增加剧集参考图等可选字段后冻结 |

现行图为 `sha256:c2a2d40869e19eaeb88bdae0c13b9d1b6c9cddf74e83dc2e5a1a51857be66259`，由 `archive_transport` 单测钉住。改四份创作 schema 使合并摘要变化时，该测试会失败：同一改动内更新测试常量，并把上一份现行图冻结到本目录和 `FROZEN_GRAPHS`。否则用该图保存的创作运行将无法同步或导出（`unknown durable archive epoch`）。

冻结图只用于归档传输的严格历史校验，不是执行合同，不迁移或改写数据库与历史任务。新增冻结必须能从 Git 历史按生成算法精确重算出摘要；未知摘要继续拒绝。无 digest 的已有 v1 仍走 `story-creation-archive-v1.schema.json`。

现场与边界：`docs/dev/fixes/FIX-BUG-20261108-局域网故事同步缺失片段结构引用.md`（6002）、`docs/dev/fixes/FIX-BUG-20261110-故事同步9f75归档纪元未冻结.md`（9f75）。
