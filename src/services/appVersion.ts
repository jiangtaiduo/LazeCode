// 应用版本号：唯一源是 Cargo.toml（Rust 侧用 env!("CARGO_PKG_VERSION")）。
//
// 前端侧由 vite.config.ts 的 define 把 npm 包版本（构建前已对齐 Cargo 版本）
// 注入为 __APP_VERSION__；直接 `vite dev` 时该宏不存在，回落开发占位。
// 之前版本号散落在 package.json / tauri.conf.json / SettingsDialog 硬编码三处
// 且互相漂移（1.0.0 vs 0.1.0），之后只认这一处。

declare const __APP_VERSION__: string | undefined;

export const APP_VERSION: string =
  typeof __APP_VERSION__ !== 'undefined' ? __APP_VERSION__ : '0.0.0-dev';
