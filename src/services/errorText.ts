// 后端错误的人类可读化（从 App.tsx 迁出，行为不变）。
//
// 两件事合一，因为它们在 50+ 个 catch 里总是成对出现：
// 1. `rawErrText(err)` —— 非 Error 的 throw（字符串、字面量）在 TS 里合法，
//    `err.message` 会是 undefined。
// 2. `describeBackendError(...)` —— 后端错误走 `ERR:<code>|<detail>` 协议
//    （见 services/i18n/errors.ts），码是 ASCII 标识，文案在前端字典。
//    不翻的话用户看到「Failed to read directory: 路径不存在: /x」这种中英混排。

import { describeBackendError } from './i18n';

/** throw 出的东西转纯文本（Error 取 message，其余 String() 化）。 */
export const rawErrText = (err: unknown): string =>
  err instanceof Error ? err.message : String(err);

/** 取 throw 出的错误的可读文本，并把后端错误码翻成当前语言。 */
export const errText = (err: unknown): string =>
  describeBackendError(rawErrText(err));

/** 后端错误串里的协议码（`ERR:<code>|...` 的 code），非协议串返回 null。 */
export const errCode = (err: unknown): string | null => {
  const raw = rawErrText(err);
  if (!raw.startsWith('ERR:')) return null;
  const idx = raw.indexOf('|', 4);
  if (idx < 0) return null;
  const code = raw.slice(4, idx);
  return code && !/\s/.test(code) ? code : null;
};
