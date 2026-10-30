const EXTERNAL_URL = /^(?:[a-z][a-z\d+.-]*:|\/\/)/i;
const ABSOLUTE_PATH = /^(?:\/|[a-z]:[\\/])/i;

/** 归一化本地路径；只处理链接解析需要的路径语义，不触碰文件系统。 */
function normalizeLocalPath(path: string): string {
  const normalized = path.replace(/\\/g, '/');
  const absolute = normalized.startsWith('/');
  const drive = normalized.match(/^[a-z]:/i)?.[0] ?? '';
  const body = drive ? normalized.slice(drive.length) : normalized;
  const parts: string[] = [];

  for (const part of body.split('/')) {
    if (!part || part === '.') continue;
    if (part === '..') {
      if (parts.length > 0 && parts[parts.length - 1] !== '..') {
        parts.pop();
      } else if (!absolute && !drive) {
        parts.push(part);
      }
      continue;
    }
    parts.push(part);
  }

  const joined = parts.join('/');
  if (drive) return `${drive}/${joined}`;
  return absolute ? `/${joined}` : joined;
}

/**
 * 判断 path 是否位于 root 子树内（root 自身也算）。root 期望为绝对路径。
 *
 * 比较前把反斜杠统一成斜杠并去掉 root 末尾斜杠；macOS 与 Windows 大小写不敏感，
 * 忽略大小写才不会把 `/Users/x/Proj` 误判成越界到 `/users/x/proj` 之外。
 */
function isWithinRoot(path: string, root: string): boolean {
  const candidate = path.replace(/\\/g, '/').toLowerCase();
  let base = root.replace(/\\/g, '/').replace(/\/+$/, '').toLowerCase();
  if (base === '') base = '/';
  // 盘根形如 `c:`，补上斜杠才能与 `c:/Users/...` 做前缀比较
  if (/^[a-z]:$/.test(base)) base = `${base}/`;
  if (base === '/') return candidate.startsWith('/');
  return candidate === base || candidate.startsWith(`${base}/`);
}

/**
 * 取越界判定用的根目录；返回 null 表示不设边界。
 * workspaceRoot 缺省时退化为 filePath 所在目录子树，至少拦住 `..` 逃逸到项目外。
 */
function containmentRoot(filePath: string, workspaceRoot?: string): string | null {
  const normalizedFilePath = filePath.replace(/\\/g, '/');
  // 文件本身在工作区外（外部挂载目录打开的文件）时不按 workspaceRoot 判定，
  // 否则它自己的本地链接会全部失效；此时退化为文件所在目录子树。
  if (workspaceRoot && isWithinRoot(normalizedFilePath, workspaceRoot)) return workspaceRoot;
  const lastSlash = normalizedFilePath.lastIndexOf('/');
  const base = lastSlash >= 0 ? normalizedFilePath.slice(0, lastSlash) : '';
  // 没有目录信息的相对文件名无法界定边界：不额外拦截
  return base ? base : null;
}

/**
 * 将 Markdown 链接解析为当前文件所在目录下的本地绝对路径，并确保结果不逃逸出工作区。
 *
 * react-markdown 的 URL 转换会把中文路径编码为 %E6...；后端文件读取
 * 需要真实文件名，因此在判断协议前先解码。查询参数和锚点不属于文件路径。
 *
 * workspaceRoot 缺省时退化为只允许 filePath 所在目录子树，至少拦住
 * `[x](../../../../.ssh/id_rsa` 这类 `..` 逃逸；给出工作区根时以它为边界。
 * 越界返回 null，让调用方退回原始 href（不打开文件）。
 */
export function resolveMarkdownPath(filePath: string, href: string, workspaceRoot?: string): string | null {
  if (!href || href.startsWith('#')) return null;

  const hashIndex = href.indexOf('#');
  const rawPathWithQuery = hashIndex >= 0 ? href.slice(0, hashIndex) : href;
  const encodedPath = rawPathWithQuery.split('?', 1)[0];
  if (!encodedPath) return null;

  let decodedPath: string;
  try {
    decodedPath = decodeURIComponent(encodedPath);
  } catch {
    return null;
  }

  if (EXTERNAL_URL.test(decodedPath) && !ABSOLUTE_PATH.test(decodedPath)) return null;

  const root = containmentRoot(filePath, workspaceRoot);

  if (ABSOLUTE_PATH.test(decodedPath)) {
    const absolute = normalizeLocalPath(decodedPath);
    return root && !isWithinRoot(absolute, root) ? null : absolute;
  }

  const normalizedFilePath = filePath.replace(/\\/g, '/');
  const lastSlash = normalizedFilePath.lastIndexOf('/');
  const base = lastSlash >= 0 ? normalizedFilePath.slice(0, lastSlash) : '';
  const resolved = normalizeLocalPath(base ? `${base}/${decodedPath}` : decodedPath);
  return root && !isWithinRoot(resolved, root) ? null : resolved;
}
