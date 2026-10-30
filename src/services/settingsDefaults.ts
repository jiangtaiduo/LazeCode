// 应用设置的默认值与类型。独立成无依赖模块，供运行时与 verify-settings 共用。
export interface AppSettings {
  themeId: number;
  /** 界面风格变体（data-variant，控制整窗主题） */
  uiVariant: number;
  terminalFontSize: number;
  editorFontSize: number;
  /** 界面字号（px，基准 13，默认 14）：通过 webview 缩放整体放大/缩小 UI 文本 */
  uiFontSize: number;
  /** macOS 交通灯 / Windows 细线三键 */
  captionStyle: 'macos' | 'windows';
  /** 界面语言（菜单/对话框/全部界面文案）。默认英文。 */
  language: 'en' | 'zh-CN';
  /** 文件树与搜索忽略的目录与文件模式（对齐 PyCharm Ignored Files and Folders） */
  ignoredPatterns: string[];
  /** 自动检查更新（默认开启，每天最多一次，可在设置关闭） */
  autoCheckUpdates: boolean;
  /** 上次检查更新时间戳（毫秒） */
  lastUpdateCheckTime: number;
  /** 用户选择忽略的版本号（如 "0.2.0"） */
  ignoredUpdateVersion: string;
}

export const DEFAULT_SETTINGS: AppSettings = {
  /** 0 = 跟随界面变体；1 = IDE 深色（编辑器配色） */
  themeId: 1,
  /** 默认界面风格：1 = macOS Native 深色 */
  uiVariant: 1,
  terminalFontSize: 13,
  editorFontSize: 13,
  uiFontSize: 14,
  captionStyle: 'macos',
  /** 默认英文（不做系统语言探测）—— 见 i18n/index.ts DEFAULT_LANGUAGE */
  language: 'en',
  ignoredPatterns: [
    '.git',
    '.idea',
    '.DS_Store',
    'node_modules',
    '.venv',
    '__pycache__'
  ],
  autoCheckUpdates: true,
  lastUpdateCheckTime: 0,
  ignoredUpdateVersion: ''
};
