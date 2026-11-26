# 提示词优化独立壳 · 官方构建与验收入口

本目录是独立壳（`com.bgxiong.prompt-optimize`，产品名「提示词优化」）的官方构建入口。主程序打包脚本与主仓 Cargo / Tauri 配置不由本入口修改。

## 官方命令

在仓库任意工作树中：

```powershell
cd apps/bgxiong-prompt-optimize
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build-official.ps1
```

`package.json` 脚本：

- `npm run build` — 前端 `tsc -b && vite build`（被 `tauri.conf.json` 的 `beforeBuildCommand` 调用）
- `npm run build:official` — 完整官方入口（`npm ci`、资源与共享 UI 隔离检查、前端构建、`cargo tauri build`）
- `npm run check:resources` — T29 资源表：只指向权威目录，目标路径不含盘符，并对 `h3-image-prompt-writing`、`generic-video-prompt-writing` 的 `SKILL.md` 做 digest
- `npm run check:shared-ui` — 跨目录共享 UI 只从 `ui/src` 源码与本壳 `node_modules` 解析，不依赖 `ui/node_modules`
- `npm run test:layout` / `npm run test:boundary`

仅验证前端与资源时：`.\scripts\build-official.ps1 -SkipNative`。

## 权威资源来源（不依赖本机偶然目录）

壳 `src-tauri/tauri.conf.json` 的 `bundle.resources` 使用仓库相对路径，构建时复制到安装物内的同名目录：

| 安装目标 | 权威源（与主程序发行一致） |
|---|---|
| `internal-prompt-skills/` | `src-tauri/crates/bgx-internal-prompts/resources/internal-prompt-skills/` |
| `builtin-agent-skills/` | `src-tauri/resources/builtin-agent-skills/` |

运行时默认在 exe / resource dir 旁查找这两个目录。开发可用绝对路径环境变量 `BGXIONG_INTERNAL_PROMPT_ROOT`；相对路径会被拒绝。用户可在设置里显式填写技能库绝对路径，默认不会去读主程序 `app_data_root.txt` 或 `story.db`。

注意：仓库根上的 `internal-prompt-skills/` 是作者区，与已打包进发行安装程序的 crate 资源并不完全相同。壳进包必须用上表 crate 路径，才能与主程序 NSIS 安装的 `$INSTDIR\internal-prompt-skills` 对齐。Skill 不复制进本应用的 git 目录。

窗口图标通过相对路径使用仓库 src-tauri/icons/icon.ico 与 icon.png，不在本应用 git 中再复制一份。


## 共享 UI

壳从 `ui/src/components/ui-chrome-textarea/standalone.ts` 引入统一控件，而不是默认 barrel。官方构建只安装本应用的 `package-lock.json`，并在 Vite 中把 `react` / `react-dom` 固定到本壳 `node_modules`。不要为了构建壳而给 `ui/` 做 `npm ci`，尤其当 `ui/node_modules` 是指向主仓的 junction 时。

## 给主程序构建（D）的接线

本入口不改主仓 `src-tauri/Cargo.toml`、主程序 `tauri.conf.json`、`installer.nsi` 或「一键打包安装包」。若开发版 / 未来发行版主程序 EXE 需要同时带上本壳，请 D 单独接线，建议：

1. 主程序自身仍按其正式入口构建；不要用壳构建代替主程序验收。
2. 主程序安装器若要附带壳，应调用本目录 `scripts/build-official.ps1` 的产物，或等价地执行同一套相对资源表，而不是改写权威 Skill 目录。
3. 主程序 `internal-prompt-skills` 仍由 NSIS 从 crate 资源安装（现有 T29 约定），不要改到 `tauri.conf.json` 的 `bundle.resources`，以免与现有主程序测试冲突。
4. 壳是独立 identifier `com.bgxiong.prompt-optimize`，不要并进主程序单实例插件。
5. 本包有意不加入主 Cargo workspace；用 `--manifest-path apps/bgxiong-prompt-optimize/src-tauri/Cargo.toml` 构建。

## Windows 打包目标

`src-tauri/tauri.conf.json` 的 `bundle.targets` 为 `["nsis"]`，而不是 `"all"`。原因：官方产品名是「提示词优化」，WiX / MSI 默认 en-US 代码页 1252 无法编码该名称（`light.exe` 报 `LGHT0311`）。第一次官方入口因此在 rustc 已成功后以退出码 1 失败；改为 NSIS 后官方入口退出码 0。NSIS 也与主程序发行形态一致。不要为了打 MSI 而改掉中文产品名。

## 验收状态

干净 Windows 开箱安装、真实 GUI 点击、付费/真实模型调用按用户要求由其人工验收，记 `NOT_RUN` / 用户自验。离线资源表、前端官方脚本与 `cargo tauri build`（NSIS）通过后，只能证明构建接线、权威目录复制和独立壳产物已生成，不能汇总成开箱或模型通过。

2026-11-16 17:08:51（Asia/Shanghai）官方入口真实退出码 0，日志 `E:\Tools\Temp\bgxiong-prompt-optimize-official-logs-20261116-nsis\summary.log`。便携 EXE：`E:\Tools\Temp\bgxiong-prompt-optimize-official-target-20261116\release\bgxiong-prompt-optimize.exe`，20,167,680 字节，SHA256 `0B4F7B4EB0FD8195B8A9C7700CF6F522A56BD94771763571CFAB3D6F1B5D9177`。技能目录已在 EXE 旁。详见 `docs/dev/reports/prompt-optimize-standalone/HANDOVER-20261116.md`。
