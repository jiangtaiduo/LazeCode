// Ghostty 原生集成服务层。
//
// 只保留 `detectGhostty`：终端已改为 canvas 自绘 + 原生 VT（libghostty-vt），
// 上一代 xterm 时代的主题映射层（toTerminalTheme / loadGhosttyConfig /
// loadGhosttyTheme / listGhosttyThemes）与 launcher（launchGhostty）随之
// 全部失去调用方，已删除。
//
// 「项目树右键 → 在外部 Ghostty 中打开」改为 App.tsx 直接 invoke
// `launch_ghostty({ cwd })`：该命令的 Rust 签名只收 cwd 一个参数
// （旧版有个可传任意命令的 `command` 参数，已被移除），不再需要这一层包装。

import { invokeCommand } from './tauriBridge';

/** Rust `GhosttyInfo` 序列化结构（serde 默认 snake_case） */
export interface GhosttyInfo {
  installed: boolean;
  version: string;
  binary_path: string;
  resources_dir: string;
  config_path: string;
  theme_count: number;
  platform_supported: boolean;
}

/** 探测本机 Ghostty 安装状态（版本、主题数量、配置路径），供设置页 About 展示 */
export async function detectGhostty(): Promise<GhosttyInfo> {
  return invokeCommand<GhosttyInfo>('detect_ghostty');
}
