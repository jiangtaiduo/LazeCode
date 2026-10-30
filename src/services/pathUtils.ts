// 纯路径小函数（从 App.tsx 迁出，行为不变，无依赖，可单测）。

/** 取父目录路径（根路径返回自身） */
export const parentOfPath = (p: string): string => {
  const trimmed = p.replace(/[\\/]+$/, '');
  const idx = Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\'));
  return idx > 0 ? trimmed.slice(0, idx) : trimmed;
};

/** 文件名（最后一段） */
export const baseNameOf = (p: string): string => p.split(/[\\/]/).pop() ?? p;

/** 相对路径 → 工作区绝对路径。git 变更清单（git status）返回仓库相对路径，
 *  而 read_file_content / openFile 等文件系统操作必须用绝对路径，否则会解析到进程 cwd 报错 */
export const toWorkspaceAbsPath = (workspace: string, p: string): string => {
  if (!p || !workspace || /^([A-Za-z]:[\\/]|\/)/.test(p)) return p;
  return `${workspace.replace(/[\\/]+$/, '')}/${p}`;
};

/** LSP 根：Git 仓库根优先；非仓库的单文件工作区取文件父目录。
 *  直接把文件路径交给语言服务器会让 Rust 侧 current_dir(file) 失败。 */
export const lspRootFor = (
  workspacePath: string,
  workspaceIsFile: boolean,
  gitRoot: string
): string => {
  if (gitRoot) return gitRoot;
  return workspaceIsFile ? parentOfPath(workspacePath) : workspacePath;
};

/** 路径简写：将 ~/ 或 /Users/<username> 或 /home/<username> 简写为 ~/（对齐 PyCharm 实测） */
export const formatTildePath = (p: string): string => {
  if (!p) return '';
  return p.replace(/^\/(?:Users|home)\/[^/]+/, '~');
};

/**
 * 获取项目字母大写徽章缩写（对齐 PyCharm AvatarUtils.initials 实测算法）
 * 1. 优先提取 PascalCase / CamelCase 单词前缀，如果长度为 2 则直接返回（如 ProductMaterialsNew -> PM）
 * 2. 否则按分隔符（空格、逗号、连字符、下划线）分词，取首词与末词的首字母（如 my-project -> MP）
 * 3. 单词项目取首字母大写（如 Welcome -> W, demo -> D）
 */
export const getBadgeInitials = (name: string): string => {
  if (!name) return 'P';
  // 1. generateFromCamelCase
  let i = 0;
  while (i < name.length && !/[a-zA-Z]/.test(name[i])) i++;
  const token = name.slice(i);
  let j = 0;
  while (j < token.length && /[a-zA-Z0-9]/.test(token[j])) j++;
  const firstWord = token.slice(0, j);

  const chars: string[] = [];
  for (let idx = 0; idx < firstWord.length; idx++) {
    const c = firstWord[idx];
    if (idx === 0 || (c >= 'A' && c <= 'Z')) {
      chars.push(c);
    }
  }
  const camel = chars.join('').slice(0, 2).toUpperCase();
  if (camel.length === 2) {
    return camel;
  }

  // 2. 尝试分隔符：空格、逗号、连字符、下划线
  for (const sep of [' ', ',', '-', '_']) {
    const parts = name.split(sep).filter(Boolean);
    if (parts.length >= 2) {
      return (parts[0][0] + parts[parts.length - 1][0]).toUpperCase();
    }
  }

  return camel ? camel : name.slice(0, 1).toUpperCase();
};

/**
 * Project avatar palette. Colors are assigned by hashing the project name, so
 * no project-name mapping (and no user data) ships in the bundle.
 */
const PROJECT_PALETTE = [
  '#9d4edd', // violet
  '#e06c43', // coral
  '#d48806', // amber
  '#3b82f6', // blue
  '#c0399f', // magenta
  '#536dfe', // indigo
  '#7cb342', // olive
  '#2bc8bb', // teal
  '#e75371'  // rose
];

export const getProjectBadgeColor = (name: string): string => {
  let h = 0;
  for (let i = 0; i < name.length; i++) h = (h * 31 + name.charCodeAt(i)) >>> 0;
  return PROJECT_PALETTE[h % PROJECT_PALETTE.length];
};
