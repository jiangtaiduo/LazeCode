// Monaco 本地打包与全局配置。
//
// 之前 @monaco-editor/react 默认从 jsdelivr CDN 拉取 monaco（版本还和 package.json
// 不一致），离线就停在 Loading...。这里改为直接使用本地依赖，并注册 worker。
//
// monaco 本体重约 4MB，而首屏（欢迎页 / 文件树 / Git 面板）完全不需要它。
// 因此这里刻意不静态 import 'monaco-editor'：模块顶层只留「无 monaco 运行时依赖」
// 的轻量内容（worker 构造器引用、主题色板数据、主题映射表），
// 所有触及 monaco 运行时的动作（loader.config、defineTheme、setTheme、
// typescript defaults、DEV 全局挂载）都收进 ensureMonacoReady() 里按需动态加载。
// 这样构建器才会把 monaco 拆成独立 chunk，首屏主包不再背着编辑器代码。
// 仅用于类型标注：色板是纯数据，运行时不应因此把 monaco 拖进主 chunk
import type * as monaco from 'monaco-editor';

/** monaco 模块命名空间的类型别名（避免在签名里写 inline import 路径） */
type MonacoInstance = typeof monaco;

/** worker 环境是否已挂载：MonacoEnvironment 挂一次就够，重复挂没有意义 */
let workersReady = false;

/**
 * 各 Monaco 主题的色板定义。
 *
 * 存为纯数据而非顶层 defineTheme 调用：主题定义在 monaco 加载前也能安全存在，
 * 由 ensureMonacoReady() 在运行时按需注册，避免一次性注册全部主题。
 */
const MONACO_THEME_DEFS: Record<string, monaco.editor.IStandaloneThemeData> = {
  'ide-jetbrains-dark': {
    base: 'vs-dark',
    inherit: true,
    rules: [
      { token: 'comment', foreground: '7a7e85', fontStyle: 'italic' },
      { token: 'keyword', foreground: 'cf8e6d' },
      { token: 'string', foreground: '6aab73' },
      { token: 'number', foreground: '2aacb8' },
      { token: 'type', foreground: '56a8f5' },
      { token: 'function', foreground: '56a8f5' },
      /* Markdown 标题（对齐 PyCharm 实测：# 号品红 #c377db、H1 文字青 #56b6c1、
         H2+ 走默认前景）—— 具体 token 由 initMonaco 里的 markdown grammar 补丁产生 */
      { token: 'punctuation.definition.heading', foreground: 'c377db' },
      { token: 'markup.heading.h1', foreground: '56b6c1' },
      { token: 'markup.heading', foreground: 'bcbec4' }
    ],
    colors: {
      'editor.background': '#1e1f22',
      'editor.foreground': '#bcbec4',
      'editorLineNumber.foreground': '#5a5d63',
      'editorLineNumber.activeForeground': '#bcbec4',
      'editorGutter.background': '#1e1f22',
      'editor.lineHighlightBackground': '#232529',
      'editor.selectionBackground': '#214283',
      'editorCursor.foreground': '#bcbec4',
      'editorIndentGuide.background1': '#313438',
      'editorWidget.background': '#2b2d30',
      'editorWidget.border': '#393b40',
      'scrollbarSlider.background': '#3a3d4199',
      // Git 差异比较与变更底色（对齐 PyCharm 实测与用户要求）
      // 1. 新增/变更行：参考图片采样墨绿 #2c4134
      'diffEditor.insertedLineBackground': '#2c4134b3',
      'diffEditor.insertedTextBackground': '#385542cc',
      'diffEditorGutter.insertedLineGutterBackground': '#2c4134cc',
      'diffEditorOverview.insertedForeground': '#3fb950',
      // 2. 删除行：浅红色底色 #482b2b
      'diffEditor.removedLineBackground': '#482b2bb3',
      'diffEditor.removedTextBackground': '#5e3436cc',
      'diffEditorGutter.removedLineGutterBackground': '#482b2bcc',
      'diffEditorOverview.removedForeground': '#f85149',
      // 3. 对角线填充
      'diffEditor.diagonalFill': '#31343866'
    }
  },

  'ide-light': {
    base: 'vs',
    inherit: true,
    rules: [
      { token: 'comment', foreground: '8a919c', fontStyle: 'italic' },
      { token: 'keyword', foreground: '9a34c4' },
      { token: 'string', foreground: '0b7500' },
      { token: 'number', foreground: '0b75c4' },
      /* 浅色下把 Xcode Light 的语义色补齐：函数/类型同色、变量偏墨，
         避免只剩 base 主题的默认蓝（与深色版的冷调不统一）。 */
      { token: 'type', foreground: '0b4f8a' },
      { token: 'type.identifier', foreground: '0b4f8a' },
      { token: 'function', foreground: '0b4f8a' },
      { token: 'variable', foreground: '1d1d1f' },
      { token: 'delimiter', foreground: '5a5a60' },
      { token: 'operator', foreground: '5a5a60' },
      /* Markdown 标题（浅色）：# 号沿用深色品红、H1 用主题蓝、H2+ 默认前景 */
      { token: 'punctuation.definition.heading', foreground: 'c377db' },
      { token: 'markup.heading.h1', foreground: '0b75c4' },
      { token: 'markup.heading', foreground: '1d1d1f' }
    ],
    colors: {
      'editor.background': '#f8f8f9',
      'editor.foreground': '#1d1d1f',
      'editorGutter.background': '#f8f8f9',
      'editorLineNumber.foreground': '#9aa0a8',
      'editorLineNumber.activeForeground': '#1d1d1f',
      'editor.lineHighlightBackground': '#eef1f6',
      'editorIndentGuide.background1': '#e4e6ea',
      'editor.selectionBackground': '#cfe4ff',
      'editorCursor.foreground': '#1d1d1f',
      'editorWidget.background': '#ffffff',
      'editorWidget.border': 'rgba(0, 0, 0, 0.12)',
      'scrollbarSlider.background': '#00000033',
      'editorBracketMatch.background': '#0071e31a',
      'editorBracketMatch.border': '#0071e3',
      'diffEditor.insertedLineBackground': '#e6f3e6cc',
      'diffEditor.insertedTextBackground': '#c5e6c5cc',
      'diffEditorGutter.insertedLineGutterBackground': '#d4ebd4',
      'diffEditorOverview.insertedForeground': '#28a745',
      'diffEditor.removedLineBackground': '#fdeeedcc',
      'diffEditor.removedTextBackground': '#fad0d4cc',
      'diffEditorGutter.removedLineGutterBackground': '#fcdfe2',
      'diffEditorOverview.removedForeground': '#dc3545',
      'diffEditor.diagonalFill': '#e4e6ea66'
    }
  },
};

/** 界面变体 / 手动选择 → Monaco 主题（id 0 表示跟随界面变体） */
/**
 * 括号配对高亮（`bracketPairColorization`）的配色兜底。
 *
 * 自定义主题都没定义 `editorBracketHighlight.*`，Monaco 就用内置默认色 ——
 * 第 1 层是亮金色 `#FFD700`，再叠加「光标旁括号的配对方框」
 * （`editorBracketMatch`，默认 always），于是代码里会突然出现一个**黄色方框**。
 * 这里按主题明暗补一套低饱和层级色：功能保留（仍能一眼看出层级与配对），
 * 观感与 IDE 一致。
 */
const BRACKET_COLORS_DARK = ['#7f8ea3', '#8ab4f8', '#a5d6a7', '#e0c080', '#c792ea', '#89ddff'];
const BRACKET_COLORS_LIGHT = ['#5f6b7a', '#3b6ea5', '#4c7a4c', '#8a6d1f', '#7a4c9e', '#2f6f7a'];

function fillBracketHighlight(def: monaco.editor.IStandaloneThemeData): void {
  const colors = (def.colors ??= {} as Record<string, string>);
  if (colors['editorBracketHighlight.foreground1']) return;
  const light = def.base === 'vs';
  (light ? BRACKET_COLORS_LIGHT : BRACKET_COLORS_DARK).forEach((c, i) => {
    colors[`editorBracketHighlight.foreground${i + 1}`] = c;
  });
  // 配对方框：默认同样是灰/金描边，改成主题色系里的淡描边
  colors['editorBracketMatch.background'] = light ? '#3b6ea51a' : '#8ab4f81a';
  colors['editorBracketMatch.border'] = light ? '#3b6ea5' : '#8ab4f8';
}

// 自定义主题都吃得到上面的兜底配色；这里只对所有定义跑一遍填充。
Object.values(MONACO_THEME_DEFS).forEach(fillBracketHighlight);

/** 界面变体 / 手动选择 → Monaco 主题（id 0 表示跟随界面变体）。
 *  只保留两套编辑器配色：1 = IDE 深色，2 = IDE 浅色。 */
export const MONACO_THEME_BY_APP_THEME: Record<number, string> = {
  1: 'ide-jetbrains-dark',
  2: 'ide-light'
};

/**
 * Unicode 警告高亮策略。
 *
 * 现状（黄色矩形框的来源）：monaco 的三档开关里 `nonBasicASCII` 已关，但
 * `ambiguousCharacters` 仍开，且 `allowedLocales` 默认只含 `_os` / `_vscode`。
 * monaco 的「同形字符」表把**全角 CJK 标点**（`：，；（）` U+FF1A/FF0C/FF1B/FF08/FF09）
 * 判成「可与 ASCII `: , ; ( )` 混淆」，于是每个中文冒号/逗号都被套上
 * `editorUnicodeHighlight.border`（暗色 #CCA700）的边框 + 底色 —— IDE 里满屏
 * 黄框，被用户当成代码报错。中文源码/注释里这些字符完全正常，不是坑。
 *
 * 因此这里把 `ambiguousCharacters` 也关掉：`nonBasicASCII` 与它同时关闭后，
 * 只剩 `invisibleCharacters`（零宽空格 / BOM / 双向控制符），那类字符才是
 * 真实的不可见攻击面。中文优先的 IDE 用这个组合。
 *
 * 键必须给全：该选项对象是整体替换语义，漏传的键会被清成 undefined（等于关闭）。
 */
export const UNICODE_HIGHLIGHT_OPTIONS: monaco.editor.IUnicodeHighlightOptions = {
  nonBasicASCII: false,
  invisibleCharacters: true,
  ambiguousCharacters: false,
  includeComments: true,
  includeStrings: true,
  allowedCharacters: {},
  allowedLocales: { _os: true, _vscode: true }
};

/** 已注册过的主题名 → true：defineTheme 是全局注册，重复注册没有意义 */
const definedThemes: Record<string, true> = {};
/** 当前生效的主题名；用于跳过重复的 setTheme */
let activeThemeName = '';
/** 已加载完成的 monaco 实例；未加载时为 null */
let monacoInstance: MonacoInstance | null = null;
/** monaco 初始化 promise：模块级缓存保证并发/重复调用只加载一次 */
let readyPromise: Promise<void> | null = null;
/**
 * 最近一次被请求的主题 id。
 * 加载期间内核还没就绪、monacoInstance 仍为 null，此时换主题无法立即生效；
 * 记下诉求，等 initMonaco 收尾时按它落地，避免编辑器停在过期主题上。
 */
let requestedThemeId = 1;

/** 注册（如未注册过）并切换到指定主题；内置主题（vs / vs-dark）无定义数据，直接切换 */
function applyTheme(instance: MonacoInstance, themeName: string) {
  const def = MONACO_THEME_DEFS[themeName];
  if (def && !definedThemes[themeName]) {
    instance.editor.defineTheme(themeName, def);
    definedThemes[themeName] = true;
  }
  instance.editor.setTheme(themeName);
  activeThemeName = themeName;
}

/** 首次真正加载 monaco。
 *
 *  这里必须动态 import：monaco 重约 4MB，首屏（欢迎页/文件树/Git 面板）根本不需要它。
 *  静态 import 会让它永远焊死进主 chunk；改成运行时 import 后构建器才会
 *  把它拆成独立 chunk，用户真正点开文件时才下载。 */
async function initMonaco(): Promise<void> {
  try {
    const instance = await import('monaco-editor'); // 例外：懒加载 4MB 编辑器内核，见函数注释
    monacoInstance = instance;
    // 必须先于任何 <Editor> mount：否则 @monaco-editor/react 的 loader 会回退到
    // 从 CDN 注入 <script>，离线环境直接卡在 Loading…
    const { loader } = await import('@monaco-editor/react');
    loader.config({ monaco: instance });

    // 上游 TS/JS monarch 把 defaultToken 设为 "invalid"（上游调试注释：「Set
    // defaultToken to invalid to see what you do not tokenize yet」）：JSX 文本
    // 子节点（中文提示语等）没有任何规则匹配 → 落到 invalid → 继承基础主题红色
    // （vs-dark #f44747），被误读成代码报错。改成 ''（普通前景色）后自动跟随各
    // 主题 editor.foreground；string.invalid / regexp.invalid 等真错误 token
    // 前缀更长、优先匹配，红色警示保留。
    // 时机必须在首个编辑器请求分词之前 —— 与上面 loader.config「先于任何
    // <Editor> mount」是同一条不变量。
    // 动态 import 例外：grammar 模块静态依赖整个 editor.api，顶层静态引入会把
    // 4MB monaco 内核焊进入口 chunk，违背本文件的懒加载拆分设计。
    const [tsGrammar, jsGrammar] = await Promise.all([
      import('monaco-editor/languages/definitions/typescript/typescript.js'),
      import('monaco-editor/languages/definitions/javascript/javascript.js')
    ]);
    // grammar 经 ambient 声明为 unknown（上游无类型）；用本文件已有的 import type
    // 断言到 IMonarchLanguage 后浅补丁，defaultToken 是 IMonarchLanguage 的合法键
    const withNeutralDefault = (lang: unknown): monaco.languages.IMonarchLanguage => ({
      ...(lang as monaco.languages.IMonarchLanguage),
      defaultToken: ''
    });
    instance.languages.setMonarchTokensProvider('typescript', withNeutralDefault(tsGrammar.language));
    instance.languages.setMonarchTokensProvider('javascript', withNeutralDefault(jsGrammar.language));

    // markdown：上游把 `#` 号与标题文字都标成 keyword（继承成橙色）。拆成
    // marker=punctuation（品红）+ H1 文字单独 token（青）、H2+ 走默认前景 ——
    // 对齐 PyCharm 标题着色。两条规则按 # 个数分流（规则表按序匹配，
    // H1 精确在前；`##` 起首组吃不到文字会落到第二条）。
    try {
      const mdGrammar = await import('monaco-editor/languages/definitions/markdown/markdown.js');
      const mdLang = mdGrammar.language as monaco.languages.IMonarchLanguage;
      const root = mdLang.tokenizer?.root;
      if (Array.isArray(root)) {
        const rules = root as Array<[RegExp, unknown]>;
        const idx = rules.findIndex(([re]) => re instanceof RegExp && re.source.startsWith('^(\\s{0,3}'));
        if (idx >= 0) {
          rules.splice(
            idx,
            1,
            [
              /^(\s{0,3})(#)((?:[^\#]|@escapes)+)((?:#+)?)/,
              ['white', 'punctuation.definition.heading', 'markup.heading.h1', 'punctuation.definition.heading']
            ],
            [
              /^(\s{0,3})(#+)((?:[^\#]|@escapes)+)((?:#+)?)/,
              ['white', 'punctuation.definition.heading', 'markup.heading', 'punctuation.definition.heading']
            ]
          );
        }
      }
      instance.languages.setMonarchTokensProvider('markdown', mdLang);
    } catch {
      // markdown grammar 拉不到就保持上游原样（标题退化为 keyword 橙），不致命
    }

    // worker 构造器与 MonacoEnvironment 也放进这里：顶层静态 import 会让入口 chunk
    // 反向静态依赖 monaco chunk，Vite 随即给 index.html 加 modulepreload，
    // 4MB 内核又被拖回首屏。放进 init 后整条 monaco 依赖只由动态 import 牵引。
    if (!workersReady) {
      const [
        { default: editorWorker },
        { default: jsonWorker },
        { default: cssWorker },
        { default: htmlWorker },
        { default: tsWorker }
      ] = await Promise.all([
        import('monaco-editor/editor/editor.worker?worker'),
        import('monaco-editor/language/json/json.worker?worker'),
        import('monaco-editor/language/css/css.worker?worker'),
        import('monaco-editor/language/html/html.worker?worker'),
        import('monaco-editor/language/typescript/ts.worker?worker')
      ]);
      self.MonacoEnvironment = {
        getWorker(_workerId: string, label: string) {
          switch (label) {
            case 'json':
              return new jsonWorker();
            case 'css':
            case 'scss':
            case 'less':
              return new cssWorker();
            case 'html':
            case 'handlebars':
            case 'razor':
              return new htmlWorker();
            case 'typescript':
            case 'javascript':
              return new tsWorker();
            default:
              return new editorWorker();
          }
        }
      };
      workersReady = true;
    }

    // 2) 语义校验在这里没有意义：Web Worker 里没有 node_modules / tsconfig，
    // 每个 import 都会被判成 "Cannot find module"。只保留语法校验，去掉假阳性。
    instance.typescript.typescriptDefaults.setCompilerOptions({
      target: instance.typescript.ScriptTarget.ESNext,
      module: instance.typescript.ModuleKind.ESNext,
      jsx: instance.typescript.JsxEmit.ReactJSX,
      allowNonTsExtensions: true,
      allowJs: true,
      esModuleInterop: true,
      skipLibCheck: true
    });
    instance.typescript.typescriptDefaults.setDiagnosticsOptions({
      noSemanticValidation: true,
      noSyntaxValidation: false,
      noSuggestionDiagnostics: true
    });
    instance.typescript.javascriptDefaults.setDiagnosticsOptions({
      noSemanticValidation: true,
      noSyntaxValidation: false,
      noSuggestionDiagnostics: true
    });

    // 开发期把 monaco 挂到全局，便于在 DevTools 里检查 markers / 语言服务状态
    if (import.meta.env.DEV) {
      (globalThis as unknown as Record<string, unknown>).monaco = instance;
    }

    // 收尾时按「最近一次请求」的主题落地：加载耗时数百毫秒，
    // 期间用户可能已在设置里换了配色，用最初的 themeId 会让编辑器停在过期主题上
    const finalThemeId = requestedThemeId;
    applyTheme(instance, MONACO_THEME_BY_APP_THEME[finalThemeId] ?? 'ide-jetbrains-dark');
  } catch (err) {
    // 加载失败时清掉缓存，允许下次调用重试；否则一次网络/解析失败会永久卡死编辑器
    readyPromise = null;
    activeThemeName = '';
    monacoInstance = null;
    throw err;
  }
}

/**
 * 确保 monaco 已加载并切到指定应用主题对应的编辑器主题。
 *
 * 幂等：并发与重复调用复用同一条加载链；目标主题已生效时零成本返回。
 * 调用方应在渲染 <Editor> 之前 await 它（见 App.tsx 的就绪门控）。
 */
export function ensureMonacoReady(themeId: number = 1): Promise<void> {
  // 先记下诉求：即使内核还在下载中，这个值也会被 initMonaco 收尾时采纳
  requestedThemeId = themeId;

  if (!readyPromise) {
    readyPromise = initMonaco();
  } else {
    const themeName = MONACO_THEME_BY_APP_THEME[themeId] ?? 'ide-jetbrains-dark';
    const loaded = monacoInstance;
    // 内核已就绪且目标主题未生效（例如设置里换了配色）：
    // 在原有加载链后串联一次注册 + 切换，不重新 import，也不丢并发语义
    if (loaded && activeThemeName !== themeName) {
      readyPromise = readyPromise.then(() => applyTheme(loaded, themeName));
    }
  }
  return readyPromise;
}

/**
 * 释放某个文件路径对应的 Monaco model。
 *
 * `@monaco-editor/react` 按 Uri 取/建 model 并在切换 path 时保留旧的（keepCurrentModel=false
 * 只在卸载时释放"当前那一个"），因此一个会话里打开过的文件会常驻在 monaco 全局注册表。
 * 真正丢弃缓冲（关标签 / 关项目 / LRU 淘汰）时调用它，释放正文 + undo 栈。
 *
 * 内核尚未加载（monacoInstance 为 null）时无事可做：那时也不存在任何 model；
 * 不要在这里 await ensureMonacoReady —— 只为释放一个 model 去拉 4MB 内核不划算。
 * 注意：不要用它做"标签切换"的清理，会丢掉 undo 历史。
 */
export function disposeModelForPath(path: string): void {
  const instance = monacoInstance;
  if (!instance || !path) return;
  instance.editor.getModel(instance.Uri.file(path))?.dispose();
}
