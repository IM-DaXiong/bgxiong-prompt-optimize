/** 画布层右键菜单 z-index（无限画布 / 肖像画布等） */
export const CONTEXT_MENU_Z_CANVAS = 10020;

/** 列表层右键菜单 z-index（场次列表、资产列表等） */
export const CONTEXT_MENU_Z_LIST = 10030;

/** 故事板内嵌画布菜单（相对画布根定位，层级低于全局 fixed 菜单） */
export const CONTEXT_MENU_Z_STORYBOARD_EMBED = 21;

/** 系统层（复制/剪贴板等，由各模块 constants 覆盖时可引用） */
export const CONTEXT_MENU_Z_SYSTEM = 10040;

/** 默认菜单最小宽度（故事板 SSOT） */
export const CONTEXT_MENU_MIN_WIDTH = 148;

/** 肖像类菜单略宽 */
export const CONTEXT_MENU_MIN_WIDTH_PORTRAIT = 168;

/** 视口边距（clamp） */
export const CONTEXT_MENU_VIEWPORT_MARGIN = 8;

/** 菜单 shell 根节点（dismiss contains / 业务侧外部点击检测 SSOT） */
export const CONTEXT_MENU_INSIDE_SELECTOR = "[data-ctx-menu-shell]";
