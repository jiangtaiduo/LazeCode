#!/usr/bin/env node
// 分层设置验证（运行: npm run verify:settings）。
//
// 覆盖 settingsLayers.ts 的全部语义：层叠优先级、非法值回落、
// 未知 key 警告、项目文件解析、VSCode 导入映射。
// 用法与其它 verify 脚本一致：node --experimental-strip-types scripts/verify-settings.mjs

import {
  resolveSettings,
  parseProjectSettingsText,
  importFromVSCode,
  projectSettingsPath,
} from '../src/services/settingsLayers.ts';

// 直接 import 生产实现，避免脚本里的镜像常量永远自洽。
import { DEFAULT_SETTINGS as DEFAULTS } from '../src/services/settingsDefaults.ts';
import { normalizeLanguage } from '../src/services/languageCore.ts';

const DEPS = { defaults: DEFAULTS, normalizeLanguage };

let failures = 0;
const assert = (cond, msg, extra) => {
  if (cond) console.log(`  ok  ${msg}`);
  else {
    failures += 1;
    console.error(`FAIL  ${msg}${extra !== undefined ? `（实际 ${JSON.stringify(extra)}）` : ''}`);
  }
};
const eq = (a, b) => JSON.stringify(a) === JSON.stringify(b);

// ---- 层叠优先级 ----
{
  const r = resolveSettings({}, DEPS);
  assert(eq(r.values, DEFAULTS), '空输入 = 默认值');
  assert(Object.values(r.sources).every((s) => s === 'defaults'), '空输入来源全是 defaults');
  assert(eq(r.warnings, []), '空输入无警告');
}
{
  const r = resolveSettings({ user: { editorFontSize: 16 } }, DEPS);
  assert(r.values.editorFontSize === 16, 'user 层覆盖 defaults');
  assert(r.sources.editorFontSize === 'user', '来源记为 user');
  assert(r.values.terminalFontSize === DEFAULTS.terminalFontSize, '未动 key 保持默认');
}
{
  const r = resolveSettings(
    { user: { editorFontSize: 14, themeId: 1 }, project: { editorFontSize: 16 }, session: { themeId: 2 } },
    DEPS,
  );
  assert(r.values.editorFontSize === 16 && r.sources.editorFontSize === 'project', 'project 覆盖 user');
  assert(r.values.themeId === 2 && r.sources.themeId === 'session', 'session 优先级最高');
}
{
  const r = resolveSettings({ user: { themeId: 99, captionStyle: 'win11' } }, DEPS);
  assert(r.values.themeId === DEFAULTS.themeId, '非法 themeId 回落');
  assert(r.values.captionStyle === DEFAULTS.captionStyle, '非法 captionStyle 回落');
  assert(eq(r.warnings, ['user:themeId:invalid-value', 'user:captionStyle:invalid-value']), '非法值警告');
}
{
  const r = resolveSettings({ project: { tabSize: 4, editorFontSize: 15 } }, DEPS);
  assert(r.values.editorFontSize === 15, '已知 key 生效');
  assert(!('tabSize' in r.values), '未知 key 不进 values');
  assert(eq(r.warnings, ['project:tabSize:unknown-key']), '未知 key 警告');
}
{
  const r = resolveSettings({ user: '[1,2,3]' }, DEPS);
  assert(eq(r.values, DEFAULTS), 'user 层非对象时整体忽略');
}
{
  const r = resolveSettings({ user: { editorFontSize: 200, terminalFontSize: NaN } }, DEPS);
  assert(r.values.editorFontSize === 48, '字号上钳 48', r.values.editorFontSize);
  assert(r.values.terminalFontSize === DEFAULTS.terminalFontSize, 'NaN 回落');
}
{
  const r = resolveSettings({ user: { language: 'fr' } }, DEPS);
  assert(r.values.language === 'en', '垃圾语言归一 en');
  const r2 = resolveSettings({ user: { language: 'zh-TW' } }, DEPS);
  assert(r2.values.language === 'zh-CN', 'zh 系归一 zh-CN');
}
{
  const r = resolveSettings({ user: { ignoredPatterns: ['.git', 'dist', '*.tmp'] } }, DEPS);
  assert(eq(r.values.ignoredPatterns, ['.git', 'dist', '*.tmp']), 'ignoredPatterns 校验');
  assert(r.sources.ignoredPatterns === 'user', '来源记为 user');
  const r2 = resolveSettings({ user: { ignoredPatterns: 'invalid' } }, DEPS);
  assert(eq(r2.values.ignoredPatterns, DEFAULTS.ignoredPatterns), '非数组 ignoredPatterns 回落');
}

// ---- 项目文件 ----
{
  const r = parseProjectSettingsText(null, DEPS);
  assert(eq(r.partial, {}) && eq(r.warnings, []), 'null 文本 = 无项目层');
}
{
  const r = parseProjectSettingsText('{"editorFontSize": 15, "tabSize": 2, "themeId": 7}', DEPS);
  assert(eq(r.partial, { editorFontSize: 15 }), '只提取有效 key', r.partial);
  assert(eq(r.warnings, ['project:tabSize:unknown-key', 'project:themeId:invalid-value']), '项目层警告');
}
{
  let threw = false;
  try {
    parseProjectSettingsText('{oops', DEPS);
  } catch {
    threw = true;
  }
  assert(threw, '损坏 JSON 抛错（调用方提示用户修文件）');
}
{
  const r = parseProjectSettingsText('[1]', DEPS);
  assert(eq(r.partial, {}) && eq(r.warnings, ['project:<root>:not-an-object']), '非对象根给警告');
}
assert(projectSettingsPath('/a/b') === '/a/b/.lazecode/settings.json', 'posix 路径拼接');
assert(projectSettingsPath('/a/b/') === '/a/b/.lazecode/settings.json', '尾斜杠归一');
assert(projectSettingsPath('C:\\a\\b') === 'C:\\a\\b\\.lazecode\\settings.json', 'windows 路径拼接');

// ---- VSCode 导入 ----
{
  const r = importFromVSCode(
    {
      'editor.fontSize': 15,
      'terminal.integrated.fontSize': 14,
      'workbench.colorTheme': 'Default Dark+',
      locale: 'zh-cn',
      'files.autoSave': 'afterDelay',
    },
    normalizeLanguage,
  );
  assert(r.partial.editorFontSize === 15, 'editor.fontSize 映射');
  assert(r.partial.terminalFontSize === 14, 'terminal 字号映射');
  assert(r.partial.themeId === 1, 'dark 主题映射');
  assert(r.partial.language === 'zh-CN', 'locale 映射');
  assert(eq(r.mapped, ['editor.fontSize', 'terminal.integrated.fontSize', 'workbench.colorTheme', 'locale']), 'mapped 清单');
  assert(eq(r.unmapped, ['files.autoSave']), 'unmapped 不静默吞');
}
{
  const r = importFromVSCode({ 'workbench.colorTheme': 'Default Light+' }, normalizeLanguage);
  assert(r.partial.themeId === 2, 'light 主题映射');
  const r2 = importFromVSCode({ 'workbench.colorTheme': 42 }, normalizeLanguage);
  assert(!('themeId' in r2.partial), '非法主题值不进 partial');
  assert(eq(r2.warnings, ['workbench.colorTheme:invalid-value']), '非法主题值警告');
}
{
  const r = importFromVSCode(null, normalizeLanguage);
  assert(eq(r.partial, {}) && eq(r.warnings, ['not-an-object']), '非对象输入给警告');
}

if (failures > 0) {
  console.error(`\n${failures} 处失败`);
  process.exit(1);
}
console.log('ok 分层设置验证通过');
