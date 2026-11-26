/**
 * 片段画布 · document.body fixed overlay z-index SSOT
 *
 * 阶梯（低→高）：
 * - IC marquee：10005（InfiniteCanvasViewport）
 * - 视频 scrubber Portal：10010（须 < CONTEXT_MENU_Z_CANVAS）
 * - 片段浮动 DDP Portal：10015（须 > scrubber、< CONTEXT_MENU_Z_CANVAS）
 * - 画布右键菜单：10020（CONTEXT_MENU_Z_CANVAS）
 * - 视频播放弹层：10025（MODAL_Z_INDEX.VIDEO_PLAYER，须 > scrubber）
 * - 生成视频工作台：10030（MODAL_Z_INDEX.VIDEO_GENERATION_WORKBENCH，须 > scrubber）
 * - 漂移闪框：12000（临时高亮，须压过瓦片与 scrubber）
 */

import { CONTEXT_MENU_Z_CANVAS } from "./context-menu/constants";

/** IC 框选 marquee 参考值（InfiniteCanvasViewport） */
export const SEGMENT_CANVAS_Z_MARQUEE_REF = 10005;

/** 视频 scrubber body fixed Portal；须严格低于画布右键菜单 */
export const SEGMENT_CANVAS_Z_VIDEO_SCRUBBER = 10010;

/** 片段浮动详情面板 body Portal；须高于 scrubber、低于画布右键菜单 */
export const SEGMENT_CANVAS_Z_DDP_FLOAT = 10015;

/** 场次列表 F11 peek；须 > DDP float、 < 画布右键菜单 */
export const SEGMENT_CANVAS_Z_SCENE_LIST_PEEK = 10016;

/** F11 toolbar shares the scene-list overlay band. */
export const SEGMENT_CANVAS_Z_TOOLBAR_PEEK = SEGMENT_CANVAS_Z_SCENE_LIST_PEEK;

/** 故事板漂移「查看」外发光闪框（imperative DOM，临时） */
export const SEGMENT_CANVAS_Z_DRIFT_FLASH = 12000;

/** 编译期断言：scrubber 不得盖住画布右键菜单 */
const _scrubberBelowContextMenu: true =
  SEGMENT_CANVAS_Z_VIDEO_SCRUBBER < CONTEXT_MENU_Z_CANVAS ? true : (false as never);
void _scrubberBelowContextMenu;

/** 编译期断言：浮动 DDP 在 scrubber 之上、画布右键菜单之下 */
const _ddpFloatBetweenScrubberAndMenu: true =
  SEGMENT_CANVAS_Z_VIDEO_SCRUBBER < SEGMENT_CANVAS_Z_DDP_FLOAT &&
  SEGMENT_CANVAS_Z_DDP_FLOAT < CONTEXT_MENU_Z_CANVAS
    ? true
    : (false as never);
void _ddpFloatBetweenScrubberAndMenu;

/** 编译期断言：场次 peek 在浮动 DDP 之上、画布右键菜单之下 */
const _sceneListPeekBetweenDdpAndMenu: true =
  SEGMENT_CANVAS_Z_DDP_FLOAT < SEGMENT_CANVAS_Z_SCENE_LIST_PEEK &&
  SEGMENT_CANVAS_Z_SCENE_LIST_PEEK < CONTEXT_MENU_Z_CANVAS
    ? true
    : (false as never);
void _sceneListPeekBetweenDdpAndMenu;
