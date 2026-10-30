// 后端错误码 -> 当前语言文案。
//
// # 背景
//
// 后端（Rust）不持有界面语言，因此错误串改为「稳定码 + 未翻译上下文」的协议：
// `ERR:path_not_found|/x/y`（协议与解析见 `src-tauri/src/error_proto.rs`）。
// 前端在这里把码查表成当前语言的句子，detail 原样插入。
//
// 这样做的收益：后端不再有 53 处硬编码中文错误串；新增语言时这些错误
// 自动跟随，不需要动 Rust。
//
// # 兜底顺序（重要）
//
// 1. 码在表里 -> 当前语言模板
// 2. 码不在表里（后端新增了码但字典没跟上）-> **原样显示整条**，包括 `ERR:` 前缀
// 3. 不是结构化串（普通文本错误）-> 原样显示
//
// 第 2 步故意露出 `ERR:` 前缀：那是一个应当被发现的问题（字典漏了 key），
// 露出协议前缀比静默显示半截句子更容易在测试/用户报告里定位。若把它吞掉，
// 用户会看到一句没有上下文的怪话，而开发者看不出是字典缺口。
//
// # 与既有 status/git section 的关系
//
// 那些 section 里已有 `saveFailed(e)` / `openDirFailed(e)` 这类「外层模板」
// （"Save failed: X"）。它们负责**动词与句式**，本模块负责**后端错误本身**。
// 典型组合：`showStatus(t().status.openDirFailed(describeBackendError(err)))`
// -> 中文「打开目录失败：路径不存在：/x」/ 英文「Failed to open directory: Path not found: /x」。

import { t } from './index';
import type { LocaleDict, LocaleFn } from './types';

// 直接引用真实字典类型，避免这份手写副本与 LocaleDict 漂移。
export type BackendErrorDict = LocaleDict['backendErrors'];

/** 结构化错误的前缀，与后端协议一致。 */
export const ERROR_PREFIX = 'ERR:';

/**
 * 把后端错误串转成当前语言的可读文本。
 *
 * @param raw 后端返回的错误串（`invoke` reject 的 message）
 * @returns 当前语言的句子；非结构化或未知码时原样返回
 */
export function describeBackendError(raw: string): string {
  if (!raw.startsWith(ERROR_PREFIX)) return raw;
  const rest = raw.slice(ERROR_PREFIX.length);
  const idx = rest.indexOf('|');
  if (idx < 0) return raw;
  const code = rest.slice(0, idx);
  const detail = rest.slice(idx + 1);
  // 与后端解析同规格：空码 / 含空白 / 非 ASCII 都不算协议
  if (!code || /\s/.test(code) || !isAscii(code)) return raw;
  // 后端码是 snake_case（`path_not_found`），字典 key 是 camelCase
  // （`pathNotFound`，与全字典其余部分一致）。在此转换而不是让字典
  // 改用 snake_case：字典的命名一致性比迁就协议更重要，而协议这一侧
  // 只有本函数一个消费者。
  const entry = (t().backendErrors as Record<string, string | LocaleFn<[string]> | undefined>)[
    snakeToCamel(code)
  ];
  if (entry === undefined) return raw;
  // 少数码无上下文（名称空 / 贮藏名非法），字典里是纯串
  return typeof entry === 'function' ? entry(detail) : entry;
}

/** `path_not_found` -> `pathNotFound`。只处理协议码，不做通用驼峰转换。 */
function snakeToCamel(code: string): string {
  return code.replace(/_([a-z0-9])/g, (_, c: string) => c.toUpperCase());
}

/** 纯 ASCII 判定（避免引入正则 flag 兼容问题）。 */
function isAscii(s: string): boolean {
  for (let i = 0; i < s.length; i++) {
    if (s.charCodeAt(i) > 0x7f) return false;
  }
  return true;
}
