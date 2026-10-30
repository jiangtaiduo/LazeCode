// 语言归一化与默认语言的单一实现。
// i18n 运行时与 verify-settings 都从这里取，避免脚本维护一份永不失败的镜像。
import type { Language } from './i18n/types';

export const DEFAULT_LANGUAGE: Language = 'en';

export function normalizeLanguage(value: unknown): Language {
  if (typeof value !== 'string') return DEFAULT_LANGUAGE;
  const v = value.trim().replace('_', '-');
  if (v === 'zh' || v.toLowerCase().startsWith('zh')) return 'zh-CN';
  if (v.toLowerCase() === 'en' || v.toLowerCase().startsWith('en-')) return 'en';
  return DEFAULT_LANGUAGE;
}
