import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// markdown 渲染管线：react-markdown 本体加上它的一整条传递依赖链
// （remark/rehype 插件、micromark 词法层、mdast/hast/unist 工具、
// vfile 与各字符实体编解码小包）。这些包只在文档预览路径上用到。
const MARKDOWN_PKG_RE =
  /node_modules\/(?:react-markdown|remark-|rehype-|micromark|mdast-util-|hast-util-|unist-util-|unified|vfile|property-information|space-separated-tokens|comma-separated-tokens|decode-named-character-reference|character-entities|trim-lines|devlop|bail|trough|is-plain-obj|zwitch|longest-streak|ccount|escape-string-regexp|html-url-attributes)/;

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  // 版本号唯一源是 Cargo.toml；发布前由 scripts/sync-version.mjs 把
  // package.json 对齐，构建时经由本宏注入前端（见 services/appVersion.ts）。
  define: {
    __APP_VERSION__: JSON.stringify(process.env.npm_package_version ?? '0.0.0-dev')
  },
  server: {
    port: 5173,
    strictPort: true
  },
  envPrefix: ['VITE_', 'TAURI_'],
  build: {
    target: 'esnext',
    minify: !process.env.TAURI_DEBUG ? 'esbuild' : false,
    sourcemap: !!process.env.TAURI_DEBUG,
    rollupOptions: {
      output: {
        // 分块用 rolldown 原生的 `advancedChunks`，不用 `manualChunks`。
        //
        // 默认策略把所有同步 import 打成一个巨型 index chunk（此前 4.4MB），
        // 任何一处源码改动都让整个文件缓存失效，且首屏必须等它全部下载解析完；
        // 而 manualChunks 在这里只作**命名建议**：rolldown 仍会按加载图把
        // 「入口与动态块都要用」的模块挪进动态块 —— 实测 React 本体被并进
        // markdown 块，于是入口为了拿 jsx-runtime 静态依赖整块 markdown 管线，
        // index.html 直接把 343KB 的 markdown 块写进 modulepreload，懒加载收益归零。
        // advancedChunks 的 groups 是硬分组（priority 高者先认领），
        // react 家族被钉在 react 块里，markdown 块就只剩动态 import 牵引。
        advancedChunks: {
          groups: [
            // Vite 的 runtime helper 必须归入入口块：rolldown 把 __vitePreload
            // 内联进「第一个需要它的 chunk」，若落进 monaco 块，入口会为了拿它
            // 静态依赖整块 4.4MB。`\0` 前缀是真实 id 的一部分。
            { name: 'index', test: /\0vite\/(preload-helper|modulepreload-polyfill)\.js/, priority: 100 },
            // react 块只收纳 react 运行时本体：路径带结尾斜杠，避免
            // `react-markdown` 之类同前缀包被误并进来。
            { name: 'react', test: /node_modules\/(react|react-dom|scheduler|use-sync-external-store)\//, priority: 40 },
            // @monaco-editor/react 轻量且被静态引用；monaco 本体必须只剩动态 import 牵引
            { name: 'monaco-react', test: /node_modules\/@monaco-editor\/react/, priority: 30 },
            { name: 'monaco', test: /node_modules\/monaco-editor/, priority: 30 },
            { name: 'markdown', test: MARKDOWN_PKG_RE, priority: 20 },
            { name: 'tauri', test: /node_modules\/@tauri-apps/, priority: 20 }
          ]
        }
      }
    }
  }
});
