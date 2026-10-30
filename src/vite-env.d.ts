/// <reference types="vite/client" />

declare module '*.css';

// monaco 的 grammar 文件没有出类型（只有空的 register.d.ts）。本文件保持无顶层
// import 的 ambient 脚本形态 —— 模块文件里的 declare module 是「增强」语义，不会
// 创建模块；language 以 unknown 透传，精确类型由 monacoSetup 顶层 import type 断言。
// 说明：monaco package.json exports 把 `monaco-editor/<x>` 映射到 `esm/vs/<x>.js`，
// 声明必须用映射后的公开 specifier。
declare module 'monaco-editor/languages/definitions/typescript/typescript.js' {
  export const language: unknown;
}
declare module 'monaco-editor/languages/definitions/javascript/javascript.js' {
  export const language: unknown;
}
declare module 'monaco-editor/languages/definitions/markdown/markdown.js' {
  export const language: unknown;
}
