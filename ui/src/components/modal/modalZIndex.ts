/**
 * modalZIndex — 全站弹窗 z-index 分层常量。
 *
 * 冻结值，各 Modal 引用常量而非 magic number。
 * 后续新增层级须在此文件登记。
 *
 * 与片段画布 fixed overlay 阶梯对齐见 segmentCanvasOverlayZIndex.ts：
 * scrubber=10010 · 画布右键=10020 · VIDEO_PLAYER 须 > scrubber。
 */
import { SEGMENT_CANVAS_Z_DDP_FLOAT, SEGMENT_CANVAS_Z_VIDEO_SCRUBBER } from "../common/overlay/segmentCanvasOverlayZIndex";

/** CanvasLinkDragPreview fixed overlay; version-history modal must sit above it. */
const CANVAS_LINK_DRAG_PREVIEW_Z = 10001;

/** In-tree canvas chrome ceiling (CanvasTileResizeSizeTip). */
const IN_TREE_CANVAS_CHROME_Z_CEILING = 10050;

/** Toast layer (Toast.tsx); sidebar float must stay below. */
const TOAST_Z_REF = 11000;

export const MODAL_Z_INDEX = {
  /** 基础弹窗层级 */
  BASE: 10100,
  /**
   * Textarea 版本历史工作台（CornerIcon ModalShell）。
   * 须 createPortal(document.body)：画布瓦片内挂载会被 promoted 连线(z=11)/端口(z=30)盖住。
   * 数值须高于 CanvasLinkDragPreview(10001)。
   */
  TEXTAREA_VERSION_HISTORY: 10100,
  /** Opening the native text workbench blocks the originating window, including its modals. */
  TEXTAREA_WORKBENCH_OPEN_LOADING: 10101,
  /** 工作台弹窗（分镜/场次/片段） */
  WORKBENCH: 10002,
  /**
   * 片段「生成视频」工作台弹层。
   * 须高于片段画布 body fixed scrubber，低于 PAD_PICKER。
   */
  VIDEO_GENERATION_WORKBENCH: 10030,
  /** 对白配音工作台（与视频工作台同级阶梯） */
  DIALOGUE_AUDIO_WORKBENCH: 10032,
  /** 自由画布配音工作台 */
  FC_AUDIO_WORKBENCH: 10033,
  /** 自由画布生图工作台 */
  FC_IMAGE_WORKBENCH: 10033,
  /** 自由画布生视频工作台 */
  FC_VIDEO_WORKBENCH: 10033,
  /** Point/box context menu above either video workbench host. */
  VIDEO_EDIT_CONTEXT_MENU: 10034,
  /** 视频高清放大工作台（与生视频工作台同级阶梯） */
  VIDEO_UPSCALE_WORKBENCH: 10033,
  /** 视频人脸精修工作台 */
  VIDEO_FACE_REFINE_WORKBENCH: 10033,
  /** 图片高清放大工作台（与生图工作台同级阶梯） */
  IMAGE_UPSCALE_WORKBENCH: 10033,
  /** 自由画布文本分段弹层 */
  FC_TEXT_SEGMENT: 10033,
  /** 自由画布文本分镜（视频片段草稿） */
  FC_TEXT_STORYBOARD: 10033,
  /** 声音智能分段：对白戏范围选择 */
  DIALOGUE_SEGMENT_MODE: 10034,
  /** Free-canvas template export/import progress (blocks canvas) */
  FC_TEMPLATE_PROGRESS: 10040,
  /**
   * Reference-audio library (manage / pick · INV-TTS-REFLIB).
   * Align PAD_PICKER: above FC/dialogue workbenches; below progress + audio edit.
   */
  TTS_REF_LIBRARY: 10050,
  /** Reference-audio library pack progress (must sit above library) */
  TTS_REF_LIBRARY_PROGRESS: 10051,
  /**
   * 音频剪辑工作台（轻量非编 · INV-AUDIO-EDIT）。
   * 须高于 FC/对白/Voice 宿主工作台，且高于参考音库（库内可二次弹出剪辑）。
   */
  AUDIO_EDIT_WORKBENCH: 10052,
  /** 翻录转唱工作台（宿主无关；与音频剪辑同层） */
  SINGING_RELYRICS_WORKBENCH: 10052,
  /** Host-neutral ordered audio/video concat workbench. */
  MEDIA_CONCAT_WORKBENCH: 10052,
  /**
   * Canvas enroll name+waveform form over the reference library (INV-TTS-REFLIB-28).
   * Must not bump TTS_REF_LIBRARY_PROGRESS (10051) or AUDIO_EDIT_WORKBENCH (10052).
   */
  TTS_REF_LIBRARY_ENROLL: 10053,
  /** 视频工作台 · 会话历史子弹层（须盖在工作台之上） */
  VIDEO_WORKBENCH_SESSION_HISTORY: 10035,
  /**
   * 通用视频播放弹层（SegmentVersionList / 历史预览等）。
   * 须高于片段画布 body fixed scrubber，低于 PAD_PICKER。
   */
  VIDEO_PLAYER: 10025,
  /** 参考图选择器 */
  PAD_PICKER: 10050,
  /** 图片灯箱（全屏预览，需在基础弹窗之上） */
  LIGHTBOX: 10100,
  /** 图片元数据弹窗（从灯箱右键打开，须盖在灯箱之上） */
  IMAGE_METADATA: 10200,
  /**
   * VendorModelPicker body portal listbox.
   * Must sit above AppModal BASE / LIGHTBOX (10100) and IMAGE_METADATA (10200);
   * below Toast (11000) / ModalProvider confirm (11001).
   */
  MODEL_PICKER_DROPDOWN: 10250,
  /**
   * Installed Content Skill listbox (body portal).
   * Above IMAGE_METADATA (10200); below shortcut hover tip (10300).
   * INV-CSKILL-LIST-03.
   */
  CONTENT_SKILL_LISTBOX: 10260,
  /**
   * INV-UI-BTN-07: Btn shortcut hover tip (body portal).
   * Must sit above TEXTAREA_VERSION_HISTORY / LIGHTBOX (10100) and IMAGE_METADATA (10200).
   */
  SHORTCUT_HOVER_TIP: 10300,
  /** 右键/复制菜单 */
  COPY_MENU: 10150,
  /**
   * RichPlain selection format bubble (body portal).
   * Above TVH/lightbox/DDP float; below COPY_MENU (INV-RICH-PLAIN-22).
   */
  RICH_PLAIN_FORMAT_BUBBLE: 10140,
  /** 主窗左下角运营弹层：高于画布，低于 Toast / 普通 Modal / mandatory Gate */
  PROMO_NOTICE: 10080,
  /**
   * 侧边栏浮动层（App Shell · SidebarFloatingLayer）。
   * 高于 in-tree 画布 chrome（HUD 100、marquee 10005、resize tip 10050、左缘 peek 10016）；
   * 低于 Toast(11000) / ModalProvider(11001) / mandatory Gate。
   */
  SIDEBAR_FLOATING: 10900,
  /** 强制更新 Gate（须高于 HTML boot splash 与业务弹层） */
  APP_UPDATE_MANDATORY_GATE: 100001,
} as const;

const _videoPlayerAboveSegmentScrubber: true =
  MODAL_Z_INDEX.VIDEO_PLAYER > SEGMENT_CANVAS_Z_VIDEO_SCRUBBER ? true : (false as never);
void _videoPlayerAboveSegmentScrubber;

const _videoWorkbenchAboveSegmentScrubber: true =
  MODAL_Z_INDEX.VIDEO_GENERATION_WORKBENCH > SEGMENT_CANVAS_Z_VIDEO_SCRUBBER ? true : (false as never);
void _videoWorkbenchAboveSegmentScrubber;

const _videoWorkbenchHistoryAboveWorkbench: true =
  MODAL_Z_INDEX.VIDEO_WORKBENCH_SESSION_HISTORY > MODAL_Z_INDEX.VIDEO_GENERATION_WORKBENCH
    ? true
    : (false as never);
void _videoWorkbenchHistoryAboveWorkbench;

const _audioEditAboveFcAudioWorkbench: true =
  MODAL_Z_INDEX.AUDIO_EDIT_WORKBENCH > MODAL_Z_INDEX.FC_AUDIO_WORKBENCH
    ? true
    : (false as never);
void _audioEditAboveFcAudioWorkbench;

const _audioEditAboveDialogueAudioWorkbench: true =
  MODAL_Z_INDEX.AUDIO_EDIT_WORKBENCH > MODAL_Z_INDEX.DIALOGUE_AUDIO_WORKBENCH
    ? true
    : (false as never);
void _audioEditAboveDialogueAudioWorkbench;

const _ttsRefLibraryAboveFcAudioWorkbench: true =
  MODAL_Z_INDEX.TTS_REF_LIBRARY > MODAL_Z_INDEX.FC_AUDIO_WORKBENCH ? true : (false as never);
void _ttsRefLibraryAboveFcAudioWorkbench;

const _ttsRefProgressAboveLibrary: true =
  MODAL_Z_INDEX.TTS_REF_LIBRARY_PROGRESS > MODAL_Z_INDEX.TTS_REF_LIBRARY
    ? true
    : (false as never);
void _ttsRefProgressAboveLibrary;

const _audioEditAboveTtsRefLibrary: true =
  MODAL_Z_INDEX.AUDIO_EDIT_WORKBENCH > MODAL_Z_INDEX.TTS_REF_LIBRARY_PROGRESS
    ? true
    : (false as never);
void _audioEditAboveTtsRefLibrary;

const _ttsRefLibraryEnrollAboveLibrary: true =
  MODAL_Z_INDEX.TTS_REF_LIBRARY_ENROLL > MODAL_Z_INDEX.TTS_REF_LIBRARY
    ? true
    : (false as never);
void _ttsRefLibraryEnrollAboveLibrary;

const _textareaVersionHistoryAboveCanvasLinkPreview: true =
  MODAL_Z_INDEX.TEXTAREA_VERSION_HISTORY > CANVAS_LINK_DRAG_PREVIEW_Z ? true : (false as never);
void _textareaVersionHistoryAboveCanvasLinkPreview;

const _textareaHistoryAboveVideoWorkbench: true =
  MODAL_Z_INDEX.TEXTAREA_VERSION_HISTORY > MODAL_Z_INDEX.VIDEO_GENERATION_WORKBENCH
    ? true
    : (false as never);
void _textareaHistoryAboveVideoWorkbench;

const _shortcutHoverTipAboveTextareaHistory: true =
  MODAL_Z_INDEX.SHORTCUT_HOVER_TIP > MODAL_Z_INDEX.TEXTAREA_VERSION_HISTORY ? true : (false as never);
void _shortcutHoverTipAboveTextareaHistory;

const _promoNoticeBelowModalBase: true =
  MODAL_Z_INDEX.PROMO_NOTICE < MODAL_Z_INDEX.BASE ? true : (false as never);
void _promoNoticeBelowModalBase;

const _promoNoticeBelowMandatoryGate: true =
  MODAL_Z_INDEX.PROMO_NOTICE < MODAL_Z_INDEX.APP_UPDATE_MANDATORY_GATE ? true : (false as never);
void _promoNoticeBelowMandatoryGate;

const _richPlainBubbleAboveTvh: true =
  MODAL_Z_INDEX.RICH_PLAIN_FORMAT_BUBBLE > MODAL_Z_INDEX.TEXTAREA_VERSION_HISTORY
    ? true
    : (false as never);
void _richPlainBubbleAboveTvh;

const _richPlainBubbleAboveLightbox: true =
  MODAL_Z_INDEX.RICH_PLAIN_FORMAT_BUBBLE > MODAL_Z_INDEX.LIGHTBOX ? true : (false as never);
void _richPlainBubbleAboveLightbox;

const _richPlainBubbleAboveFcTemplate: true =
  MODAL_Z_INDEX.RICH_PLAIN_FORMAT_BUBBLE > MODAL_Z_INDEX.FC_TEMPLATE_PROGRESS
    ? true
    : (false as never);
void _richPlainBubbleAboveFcTemplate;

const _richPlainBubbleAboveDdpFloat: true =
  MODAL_Z_INDEX.RICH_PLAIN_FORMAT_BUBBLE > SEGMENT_CANVAS_Z_DDP_FLOAT ? true : (false as never);
void _richPlainBubbleAboveDdpFloat;

const _richPlainBubbleBelowCopyMenu: true =
  MODAL_Z_INDEX.RICH_PLAIN_FORMAT_BUBBLE < MODAL_Z_INDEX.COPY_MENU ? true : (false as never);
void _richPlainBubbleBelowCopyMenu;

const _richPlainBubbleBelowShortcutTip: true =
  MODAL_Z_INDEX.RICH_PLAIN_FORMAT_BUBBLE < MODAL_Z_INDEX.SHORTCUT_HOVER_TIP
    ? true
    : (false as never);
void _richPlainBubbleBelowShortcutTip;

const _sidebarFloatingAboveInTreeCanvasChrome: true =
  MODAL_Z_INDEX.SIDEBAR_FLOATING > IN_TREE_CANVAS_CHROME_Z_CEILING ? true : (false as never);
void _sidebarFloatingAboveInTreeCanvasChrome;

const _sidebarFloatingBelowToast: true =
  MODAL_Z_INDEX.SIDEBAR_FLOATING < TOAST_Z_REF ? true : (false as never);
void _sidebarFloatingBelowToast;

const MODAL_PROVIDER_CONFIRM_Z = 11001;

const _modelPickerDropdownAboveBase: true =
  MODAL_Z_INDEX.MODEL_PICKER_DROPDOWN > MODAL_Z_INDEX.BASE ? true : (false as never);
void _modelPickerDropdownAboveBase;

const _modelPickerDropdownAboveLightbox: true =
  MODAL_Z_INDEX.MODEL_PICKER_DROPDOWN > MODAL_Z_INDEX.LIGHTBOX ? true : (false as never);
void _modelPickerDropdownAboveLightbox;

const _modelPickerDropdownAboveImageMetadata: true =
  MODAL_Z_INDEX.MODEL_PICKER_DROPDOWN > MODAL_Z_INDEX.IMAGE_METADATA ? true : (false as never);
void _modelPickerDropdownAboveImageMetadata;

const _modelPickerDropdownBelowToast: true =
  MODAL_Z_INDEX.MODEL_PICKER_DROPDOWN < TOAST_Z_REF ? true : (false as never);
void _modelPickerDropdownBelowToast;

const _modelPickerDropdownBelowConfirm: true =
  MODAL_Z_INDEX.MODEL_PICKER_DROPDOWN < MODAL_PROVIDER_CONFIRM_Z ? true : (false as never);
void _modelPickerDropdownBelowConfirm;

const _contentSkillListboxAboveImageMetadata: true =
  MODAL_Z_INDEX.CONTENT_SKILL_LISTBOX > MODAL_Z_INDEX.IMAGE_METADATA ? true : (false as never);
void _contentSkillListboxAboveImageMetadata;

const _contentSkillListboxBelowShortcutTip: true =
  MODAL_Z_INDEX.CONTENT_SKILL_LISTBOX < MODAL_Z_INDEX.SHORTCUT_HOVER_TIP ? true : (false as never);
void _contentSkillListboxBelowShortcutTip;
