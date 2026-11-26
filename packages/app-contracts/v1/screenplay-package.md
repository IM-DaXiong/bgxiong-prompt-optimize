# screenplay-package.json · 字段说明

权威数字与闭集词表只在同目录 [`screenplay-package.json`](./screenplay-package.json)。本文件只解释「键干什么、谁消费、超限怎样」；**不要**把这里的句子当成第二份上限。

改上限：只改 JSON。`src-tauri/build.rs` 编进 Rust const，前端 [`ui/src/domain/screenplay-import/constants.ts`](../../../ui/src/domain/screenplay-import/constants.ts) 相对路径 import 同一文件。

改外部 Skill 中文条款 / 金样结构：[`src-tauri/src/screenplay_package/template.rs`](../../../src-tauri/src/screenplay_package/template.rs) 的 `instructions()`。外部导入合同不规定创作数量或要求使用特定 Skill。

改应用内一键直出分阶段怎么教模型：[`src-tauri/src/ai_prompts/builtins/screenplay_*.rs`](../../../src-tauri/src/ai_prompts/builtins/)。那五份 **不读** 本 md。

不要在 JSON 里加 `//` 或 `$comment`：内置生成器读取整份产品合同；外部 kind=`limits` 只返回导入传输预算；正式字段 Schema 走 kind=`schema`。

---

## schemaVersion

合同文件自身版本（本 JSON 的结构版本）。现在必须是 `1`。`build.rs` 校验；对不上则编译失败。

它和目录名 `packages/app-contracts/v1/` 一样属于**应用合同目录版本**，与剧本包 BGX 格式版本无关：目录/合同的 `v1` 不表示 BGX v1。

---

## bgxSchemaVersion

BGX 剧本包格式版本，唯一合法值 `5`。剧本包 `BGX-META.schemaVersion` 必须等于它；正式 JSON Schema [`screenplay-package-bgx-v5.schema.json`](./screenplay-package-bgx-v5.schema.json) 的 `const` 与之绑定。v1–v4 包一律拒绝、不自动升级；v4 是显式跳过版本，本仓没有 v4 Schema、DTO 或升级器。见下文「BGX v5 格式合同 · 版本与唯一报错」。

---

## limits

本节同时记录既有内置生成器的默认预算和导入传输预算。外部导入不执行集数、场数、镜头数或创作时长门控；`files.screenplay.contract.get(kind=limits)` 仅返回真实导入资源预算。素材、提示词和时长可以按 schema 缺省，引用必须存在，禁止截断或静默丢镜。

| 键 | 干什么 |
|---|---|
| `maxChaptersPerPackage` | 内置生成器的集数预算，不是外部导入限制。 |
| `maxScenesPerChapter` | 内置生成器的每集场数预算，不是外部导入限制。 |
| `maxShotsPerScene` | 内置生成器的每场镜数预算，不是外部导入限制。 |
| `maxShotsPerPackage` | 内置生成器的总镜头预算，不是外部导入限制。 |
| `maxSourceBytes` | 导入 / 预览源文最大字节，防大包卡死 UI 与内存。 |
| `parseTimeoutMs` | Fountain 兼容子集 + BGX 解析超时。 |
| `previewShotsPerPage` | 预览 IPC 镜头摘要每页条数，避免一次回传全包 prompt。 |
| `maxRepairRoundsPerStage` | 一键直出单阶段 LLM 格式修复次数上限（不含首次生成）。 |
| `defaultRepairRoundsPerStage` | 工作台默认修复次数。 |
| `minEpisodeDurationSeconds` | S2 规划的单集 `durationSeconds` 下限。 |
| `maxEpisodeDurationSeconds` | 同上，上限。 |
| `minChapterTargetDurationSeconds`（`content-production.json`） | 大纲每集 `durationSeconds` 写入 `targetDurationSeconds` 的下限。共用补参 `durationSecondsPerChapter` 仍用 `minEpisodeDurationSeconds`。 |
| `minAvgShotSeconds` | 工作台「平均镜长」输入下限。 |
| `maxAvgShotSeconds` | 同上，上限。 |
| `minShotDurationSeconds` | 内置生成器建议的每镜时长下限，外部导入不执行此门控。 |
| `maxShotDurationSeconds` | 内置生成器建议的每镜时长上限，生成时以所选模型能力为准。 |

创作文本字段不设字符上限，角色说明、肖像/道具/环境/静帧/视频提示词完整接收并保留。仍校验字段类型、必填结构与引用合同，不按字数拒收或截断。

---

## draftStore

一键直出长链路中途草稿，落 `{app_data}/screenplay_pipeline_drafts/`。这是草稿配额，不是剧本体上限。超限淘汰旧草稿。

| 键 | 干什么 |
|---|---|
| `maxTasks` | 同时保留多少条任务草稿。 |
| `maxTotalBytes` | 草稿目录总字节上限。 |
| `ttlDays` | 草稿过期天数。 |

---

## enums

工作台闭集。改词表等于改产品选项，须同步 UI snapshot / 门禁。合法值以 JSON 为准。

| 键 | 干什么 |
|---|---|
| `inputInterpretation` | 用户正文怎么读：自动 / 一句话设定 / 小说 / 普通剧本。 |
| `adaptationFidelity` | 改编忠实度：忠实 / 平衡 / 创意。 |
| `shotDensity` | 分镜密度：省镜 / 标准 / 细拆。 |
| `importDepth` | 生成深度：只要分镜正文 / 加静帧 / 加视频（无静帧走 T2V）/ 静帧+视频。工作台用静帧、视频两个复选投影到这些值。 |
| `targetMode` | 物化目标：新故事 / 追加到当前故事。 |
| `exportMode` | 导出：旁路原件 / 当前结构。 |
| `templateKind` | `get_screenplay_package_template` 的 kind：`fountain`/`golden` 金样、`schema` 正式 BGX v5 JSON Schema（`screenplay-package-bgx-v5.schema.json`）、`limits` 本合同 JSON 原文（limits/enums/stages）、`instructions` 外部 Skill。 |

---

## stages

一键直出长链路阶段名（合同标识，不是用户可见文案）。底栏 / Tasks 须走人话，禁止回显这些机码。

| 值 | 干什么 |
|---|---|
| `s0Parse` | 解析已有 Fountain+BGX（标准包零 LLM 路径）。 |
| `s1Facts` | 冻结故事事实。 |
| `s2Chapters` | 规划剧集。 |
| `s3Scenes` | 按集规划场次与文学剧本。 |
| `s4Shots` | 按集生成镜头语义（景别/机位等）。 |
| `s5Prompts` | 按集补 `stillPrompt` / `videoPrompt` / `durationSec`。 |
| `s6Assemble` | Rust 组包并校验。 |

---

## BGX v5 格式合同

块体字段闭集、类型、必填、枚举与数值范围只在 [`screenplay-package-bgx-v5.schema.json`](./screenplay-package-bgx-v5.schema.json)（JSON Schema Draft 2020-12）。本节只写 JSON Schema 表达不了的块顺序、唯一性、跨块引用、prompt 语义与画幅职责。解析与校验 SSOT 仍是 `src-tauri/src/screenplay_package/`。

### Schema 怎么用

- 可先导入文学稿。角色外形、定妆提示词、道具外观、环境提示词、静帧提示词、镜头时长、画幅和风格可缺省。没有镜头时保留场次正文。演员图、环境图和模型配置可在导入后补齐。
- 追加到已有故事时，省略角色外观、简介、定妆提示词或道具外观提示词会保留已有内容。
- MCP 使用 `files.screenplay.contract.get` 读取格式，用 `files.screenplay.preview` 检查源文，再通过 `files.change.prepare` 的 `files.screenplay.import` 动作与 `change.apply` 提交。可选 `clipGraph` 接受本次要创建的片段图节点和引用连线，回执返回工作区及本次节点的 ID。未传图时只导入剧本。

- 根对象恰好一个键，键名即块标记：`===BGX-PROP===` 的块体按 `properties["BGX-PROP"]` 校验。块闭集：`BGX-META`、`BGX-ROLE`、`BGX-PROP`、`BGX-SCENE`、`BGX-SHOT`。
- 所有对象 `additionalProperties:false`。旧键 `conceptPlate`、`SCENE.props` 在 v5 中是未知键，直接拒绝，不设别名。
- 必填文本必须含非空白字符（`pattern: \S`，与 Rust `trim().is_empty()` 为假同义）。
- 可选自由文本（`episodeTitle`、`sceneEnv.weather`、`title`、`beat`、`videoPrompt`、PROP 的 `material` 等）省略与空字符串等价。
- 可选枚举（`angle`、`move`、`coverage`、`dialogueMode`、`subjectKind`）写了就必须是合法值，不接受空字符串；不用就省略。`move` 省略按 `static`。
- `dialogueLines[].speaker` 省略与 `null` 等价。
- 节点上的 `x-bgxSsot` 注解标明该数字/枚举的权威来源：`screenplay-package.json#/limits/<key>`、`screenplay-package.json#/bgxSchemaVersion` 或 `rust:<路径>` 闭集。门禁逐项比对 Schema 值与来源；改数字先改本 JSON，再同步 Schema 同一节点，任何一边单改都算门禁失败。

### 版本与唯一报错

- 只接受 `BGX-META.schemaVersion` 等于 `bgxSchemaVersion`。解析器先以通用 JSON 读出 META 的 `schemaVersion`；不相等（含 1、2、缺失、非整数）时只返回下面这一条 finding 并停止解析其余块，避免旧包刷出一串未知键错误：
  - code `SCREENPLAY_SCHEMA_VERSION`，pointer `/BGX-META/schemaVersion`
  - 用户文案：「剧本包格式版本过旧或不受支持：本应用只接受 BGX v5 制作包。」
  - 修复建议：「请按当前导入 schema 修正剧本包后重新预览；使用 Skill 是可选的。」
  - 日志 `logDetail`：`schemaVersion=<实际值|missing>`
- 禁止自动升级、补 `roleBrief` / prompt、从 `portraitPrompt` 反推人物简介、把角色四字段拼成 `portraitPrompt`、把道具名扩成 `appearancePrompt`。

### 块顺序

1. `BGX-META` 恰好一个，且是全文第一个块。
2. 资产区＝META 之后、第一个 `# 集标题` 之前，只放 `BGX-ROLE` 与 `BGX-PROP`。二者可交错；模板按先 ROLE 后 PROP 输出。
3. 每个场标后紧跟一个 `BGX-SCENE`，其后是本场的 `BGX-SHOT`。
4. ROLE/PROP 出现在资产区外、SCENE/SHOT 出现在资产区内、重复 META、未知块标记，均报带行号的 finding。

### 唯一性与引用

所有比较都是逐码点精确相等，不 trim、不改大小写；唯一例外是 R3 明写的规范化。

| 规则 | 内容 |
|---|---|
| R1 | `ROLE.name` 包内唯一。Schema 禁止名字首尾空白，因此不存在「trim 后相等」的第二种解释。 |
| R2 | `PROP.id` 包内唯一，格式见 Schema `$defs/propId`（形如 `P001`）。同一实体跨集复用同一 id；id 只是包内引用键，不是数据库 id。 |
| R3 | `PROP.name` 按故事道具库的同一规范化（去除全部空白并转小写，即 `story_prop::repo::normalize_prop_title`）后包内唯一。同名不同实体须在 name 上区分（如「怀表（父亲的）」）并使用不同 id；否则导入按名合并为一行，prompt 会被静默覆盖。 |
| R4 | `SCENE.propIds` 每项必须是已声明的 `PROP.id`；数组内不重复。 |
| R5 | `SHOT.propIds` 必须是本场 `SCENE.propIds` 的子集；数组内不重复。 |
| R6 | 每个已声明的 PROP 至少被一个 `SCENE.propIds` 引用；孤立道具拒绝（保证 current 导出可逐一还原）。 |
| R7 | `SCENE.chars` ⊆ 已声明 `ROLE.name`；`SHOT.chars` ⊆ 本场 `SCENE.chars`；speaker 规则见现行 SPEC「声音身份闭集」。 |
| R8 | `SHOT.id` 场内唯一。 |

### 人物简介与视觉字段分层

`ROLE.roleBrief` 是人物叙事与表演上下文，承载身份、性格、关系、背景、说话特点和可选的外形概览；它不是生图提示词。系统生成时复用角色简介唯一 rider，按既定段落顺序只写有内容的部分，不用“未知/不详/待定”占位。代码只校验非空、类型与长度，不靠关键词或正则判断语义。

`age/facialFeatures/hair/outfit` 是角色视觉合同权威，`portraitPrompt` 是单人定妆资产图全文。三层禁止互相反推；若叙事简介的外形概览与视觉合同冲突，生图以视觉合同为准。

### 四类 prompt 语义

| 字段 | 生成什么 | 应写 | 不得写 |
|---|---|---|---|
| `ROLE.portraitPrompt` | 单角色、静态定妆资产图 | 身份外观、脸、发型、服装、标准姿态、表情、中性布光 | 剧情、对白、场景名、其他角色 |
| `PROP.appearancePrompt` | 单一道具的资产参考图 | 形制、材质、颜色、尺寸感、关键细节 | 人物、剧情动作、场景叙事 |
| `SCENE.environmentPrompt` | 无人空境 / 场景概念图 | 空间、陈设、材质、时间天气、光线、稳定布局锚点 | 人物、表演、对白、剧情因果 |
| `SHOT.stillPrompt` / `SHOT.videoPrompt` | 逐镜静帧 / 按需视频 | 本镜可见状态、动作、光线、构图 | 整段复制资产 prompt |

四类（五个字段）一律画幅中立：不写比例、分辨率、像素、横竖屏、画布尺寸，也不写厂牌、模型或 URL 参数；可以写主体相对位置、留白方向、前中后景等构图语义。这些是作者模型与结构化语义评审的约束，代码不扫描自由文本判定语义（不做关键词/正则检查）。

### 画幅职责

- `META.aspectRatio` 是包级制作意图与导入默认值；工作台 generation params / session snapshot 是提交时唯一的结构化画幅来源。
- 用户在工作台切换画幅只改提交参数，任何 prompt 正文逐字不变；代码不得从 prompt 解析或推断画幅。

### 导入落点（摘要，详见现行 SPEC）

`roleBrief` → `RoleContentV1.roleBrief`；`portraitPrompt` → `RoleContentV1.portraitPromptManual`；`PROP` → 当前故事 `story_props`（`appearancePrompt` → `visualSeed.prompt`）；`environmentPrompt` → 场次正文 `AI_PROMPT`；`stillPrompt` / `videoPrompt` → 既有分镜/视频 SSOT。全部逐字写入同一事务，导入期不调用 LLM。
