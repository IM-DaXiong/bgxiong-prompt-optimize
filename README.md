# 比格熊提示词优化

比格熊官网 [www.bgxiong.com](https://www.bgxiong.com)｜微信公众号：天途影像｜[比格熊开发治理 Skill](https://www.bgxiong.com/goods/9p65SRB2Sl)

**换一个生图或视频模型，原稿可以接着用。选好目标，比格熊按这个模型的写法重写提示词，生成前就能对照检查。**

一段在上个模型里用得顺手的提示词，换个模型可能就漏掉动作顺序，或者把要保留的画内文字改了。等预览或成片出来才发现得回去重写，时间和算力都已经花了。比格熊提示词优化把改稿提到生成之前。粘贴一句创意、镜头稿或旧提示词，选定目标，工具就会加载对应的写法 Skill，整理出可编辑、可复制的成稿。

## 哪些返工可以提前省掉

- **换模型时重复改稿。** 原稿留在左侧，可以反复切换目标，分别生成对应写法。比如同一个雨夜公交车场景，给 Qwen Image 2.1 时重点交代画面主体、空间和站牌文字，给 Seedance 时重点交代车辆经过站牌的先后动作和镜头描述。
- **画面和台词被顺手改掉。** 写法约束会提示文本模型保留人物关系、动作顺序、原话和指定文字。在支持参考图的入口，可以明确哪些地方要改、哪些细节要留。生成前直接对照原稿，改好再复制。
- **每次都得重新交代写作规则。** 专用 Skill 随目标自动选用。视频优化可分阶段整理内容、审阅并转换为目标格式，帮助发现成稿里的遗漏。官方 Skill 可以从设置里检查更新，按需安装。
- **已有的文本模型用不上。** 目标模型和改写用的文本模型可以独立选择。可以用已有的云端文本服务为另一家的图片或视频模型写提示词，也可以接自己的兼容服务或本地模型。

## 当前兼容的提示词格式

开发版提供 **13 个优化目标**，包括 4 个图片目标、8 个视频目标和 1 个文学写作目标。下列格式用于输出提示词，图片和视频生成在各自的工具里完成。

### 图片

- **Qwen Image 2.1**（`image/qwen-image-2.1`）默认把文生图原稿整理成英文画面描述，保留指定的画内文字和语言。
- **豆包 Seedream 5.5**（`image/seedream-5.5`）沿用用户语言，理顺主体、构图、光影和画内文字。其参考图写法区分单图编辑的改动范围与多图合成的素材用途。
- **MiniMax H3-Image**（`image/h3`）按主体、空间、构图和光线整理文生图。其参考图写法用 `<Picture 1>` 和 `<Subject 1>` 等编号交代来源、对象以及保留关系。
- **通用图片**（`image/general`）可用于整理其他图像模型的提示词。独立窗口在「通用图片 → 其他选项」提供参考图和可选写法 Skill。三个专用模型目标的快捷入口接收纯文本。

### 视频

当前独立窗口和 CLI 的快捷目标支持**纯文生视频的提示词改写**。首帧、首尾帧和多参考素材模式通过集成端提供素材，具体以接入目标声明的模式为准。

- **MiniMax 海螺 H3**（`video/h3@1`）采用分段结构，分开组织主体、场景概述、动作、镜头与声音。对白、歌词和画面内文字按原语言处理。
- **Seedance 2.0 系列**（`video/seedance@1`）按主体和动作时序组织镜头描述，把已有的动作与运镜安排写到对应段落。
- **可灵 Kling Classic、Kling Omni v1、Kling Omni v2**（`video/kling.classic@1`、`video/kling.omni@1`、`video/kling.omni@2`）各用对应档案，将主体、动作、镜头和原话整理为所选版本的表达。
- **万相 Wan**（`video/wan@1`）沿时间顺序组织主体动作和镜头描述，保留原稿明确给出的对白和声音用途。
- **LTX-2.5 段落格式**（`video/ltx@1`）把动作、镜头、光线与声音按先后写进连贯段落，适合连续描述。
- **通用视频**（`video/generic@1`）以自然语言交代画面、动作、时序和原话，方便带到其他视频工具。

### 文学与创作提示词

**文学提示词**（`literary/general`）可整理剧情片段、分镜写作要求和需要交给文本模型的创作指令。也可以按用途挑选已安装的写法 Skill。

## 哪些文本模型可以负责改写

- **云端**。支持 OpenAI 兼容服务、火山方舟、DeepSeek、自定义 OpenAI 兼容接口，以及通义千问和 MiniMax-M3。可配置接口地址、API 密钥和模型，并保存常用模型供下次切换。
- **本地**。支持 GGUF 文本模型，需准备模型文件和兼容的 `llama-server` 运行时。
- **第三方集成**。可载入具备文本生成能力的适配包，也可通过 SDK 回调，让调用方使用自己的模型接口。程序可查询可用目标、提交优化，并读取结果所用的目标、Skill 标识和内容摘要。[SDK 接入协议](docs/sdk/v1.md)

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

允许个人使用，也可以在自己的商业项目中使用和修改本工具，用它制作、交付项目成果。许可覆盖的原版、改版或重新打包版本均不得单独出售、作为付费功能提供，或嵌入、预装、捆绑到收费产品中交付。免费再分发须保留原版权和许可声明。[自定义使用许可](LICENSE)

官方创作 Skill 沿用内容仓的许可。仓内同步的主程序文件可按输入清单追溯来源。[官方创作 Skill 许可](https://github.com/IM-DaXiong/bgxiong-content-skills/blob/main/LICENSE) · [主程序输入清单](deps/host-inputs.lock.json)

## 关于比格熊

比格熊官网 [www.bgxiong.com](https://www.bgxiong.com)｜微信公众号：天途影像｜[比格熊开发治理 Skill](https://www.bgxiong.com/goods/9p65SRB2Sl)
