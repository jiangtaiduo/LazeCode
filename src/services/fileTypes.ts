// 文件类型判断与 Monaco 语言映射：编辑器、状态栏、桥接层共用的唯一来源。

const LANGUAGE_BY_EXTENSION: Record<string, string> = {
  go: 'go', py: 'python', ts: 'typescript', tsx: 'typescript',
  js: 'javascript', jsx: 'javascript', rs: 'rust', json: 'json',
  md: 'markdown', css: 'css', scss: 'scss', less: 'less', html: 'html', sh: 'shell',
  bash: 'shell', zsh: 'shell', yml: 'yaml', yaml: 'yaml', toml: 'ini',
  sql: 'sql', c: 'c', h: 'c', cpp: 'cpp', cc: 'cpp', java: 'java',
  rb: 'ruby', php: 'php', xml: 'xml', svg: 'xml', lock: 'plaintext',
  // Monaco 没有独立的 vue/svelte/astro 语言，用 html 分词器（SFC 的 template/script/style 都能着色）
  vue: 'html', svelte: 'html', astro: 'html',
  kt: 'kotlin', kts: 'kotlin', swift: 'swift', dart: 'dart', lua: 'lua',
  graphql: 'graphql', gql: 'graphql', dockerfile: 'dockerfile',
  m: 'objective-c', mm: 'objective-c',
  // 更多 Monaco 内置语言（diff 页与编辑器共用本表，覆盖常见工程文件）
  ps1: 'powershell', psm1: 'powershell', bat: 'bat', cmd: 'bat',
  r: 'r', scala: 'scala', jl: 'julia', pas: 'pascal', dpr: 'pascal', pp: 'pascal',
  pl: 'perl', pm: 'perl', pug: 'pug', jade: 'pug',
  hcl: 'hcl', tf: 'hcl', tfvars: 'hcl', sol: 'sol',
  ini: 'ini', cfg: 'ini', conf: 'ini', properties: 'ini',
  vb: 'vb', vbs: 'vb', cs: 'csharp',
  hpp: 'cpp', hh: 'cpp', hxx: 'cpp', cxx: 'cpp', cjs: 'javascript', mjs: 'javascript',
  jsonc: 'json', json5: 'json', mts: 'typescript', cts: 'typescript',
  ejs: 'html', erb: 'ruby'
};

/** 无扩展名 / 扩展名不表意的文件名 → Monaco 语言 */
const LANGUAGE_BY_BASENAME: Record<string, string> = {
  dockerfile: 'dockerfile',
  'containerfile': 'dockerfile',
  gemfile: 'ruby',
  rakefile: 'ruby',
  '.bashrc': 'shell',
  '.zshrc': 'shell',
  '.profile': 'shell',
  '.bash_profile': 'shell',
  '.bash_aliases': 'shell'
};

/** 走图片预览、不读取文本内容的扩展名 */
const IMAGE_EXTENSIONS: Record<string, true> = {
  png: true, jpg: true, jpeg: true, gif: true, webp: true, ico: true, bmp: true, avif: true, svg: true
};

export function extensionOf(path: string): string {
  const name = path.split(/[\\/]/).pop() ?? path;
  const dot = name.lastIndexOf('.');
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : '';
}

/** 文件是否走图片预览（不读取文本内容） */
export function isImageFile(path: string): boolean {
  return IMAGE_EXTENSIONS[extensionOf(path)] === true;
}

/** 文件是否走 Markdown 预览模式 */
export function isMarkdownFile(path: string): boolean {
  return languageOf(path) === 'markdown';
}

/** 依据文件名/扩展名推断 Monaco 语言（Dockerfile、.bashrc 等无扩展名文件先查 basename） */
export function languageOf(path: string): string {
  const name = (path.split(/[\\/]/).pop() ?? path).toLowerCase();
  const byBase = LANGUAGE_BY_BASENAME[name];
  if (byBase) return byBase;
  return LANGUAGE_BY_EXTENSION[extensionOf(path)] ?? 'plaintext';
}
