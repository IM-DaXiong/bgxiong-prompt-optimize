# 比格熊提示词优化

比格熊官网 [www.bgxiong.com](https://www.bgxiong.com)｜微信公众号：天途影像｜[比格熊开发治理 Skill](https://www.bgxiong.com/goods/9p65SRB2Sl)

独立的提示词优化工具。选择提示词要适配的目标模型，输入原始提示词，生成优化结果。结果可编辑和复制。目标模型决定写法，文本模型负责改写，可以分别配置。

## 使用和接入

- **Windows 桌面工具**。打开开发运行目录里的 `bgxiong-prompt-optimize.exe`，在「设置」选好文本服务和文本模型，再选择目标模型并优化。可在设置里检查 Skill 更新。
- **程序接入**。启动 `bgxiong-prompt-optimize-cli.exe --stdio`，按 UTF-8 JSON Lines 协议收发消息。通过 `callback` 模式接入自己的文本模型，或通过 `configured` 模式使用工具内保存的模型设置。[SDK 协议](docs/sdk/v1.md) · [Python 示例](examples/python/optimize.py) · [Rust 示例](examples/rust/src/main.rs)

## Windows 构建

在 Windows x64 环境准备 Git、Python 3.11+、Node.js 22.12+ 和 npm、Rust MSVC 工具链、Tauri CLI 2、Visual Studio C++ 构建工具和 WebView2。先提交本次构建所用源码，再从仓库根运行。

```powershell
$runtimeDir = Join-Path $env:TEMP ("bgxiong-prompt-optimize-" + (New-Guid).ToString())
& ./apps/bgxiong-prompt-optimize/scripts/build-official.ps1 -Development -RuntimeDirectory $runtimeDir
python ./examples/python/optimize.py --cli (Join-Path $runtimeDir "bgxiong-prompt-optimize-cli.exe") --self-test
```

构建脚本在指定目录写入窗口 EXE、CLI、Skill 资源、协议文件和组件校验清单。运行或复制时保留整套目录结构。[项目交付路径](RELEASE_OUTPUTS.md)

## 许可

允许个人使用、修改和商用集成，也可随自己的收费产品交付。禁止单独出售本许可覆盖的原版或改版，包括作为付费包、付费镜像或付费组件库提供。免费再分发须保留原版权和许可声明。[自定义使用许可](LICENSE)

官方创作 Skill 沿用内容仓的许可。仓内同步的主程序文件可按输入清单追溯来源。[官方创作 Skill 许可](https://github.com/IM-DaXiong/bgxiong-content-skills/blob/main/LICENSE) · [主程序输入清单](deps/host-inputs.lock.json)

## 关于比格熊

比格熊官网 [www.bgxiong.com](https://www.bgxiong.com)｜微信公众号：天途影像｜[比格熊开发治理 Skill](https://www.bgxiong.com/goods/9p65SRB2Sl)
