// 字典运行时自检：key 必须是 ASCII、en/zh 结构一致。
// 翻译过的 key（如 `文件: 'Files'`）会让 t().search.tabFiles 在另一边落空，
// tsc 只检查类型，不阻止有人两边一起改错，所以这里按运行时对象再查一次。
import { en } from '../src/services/i18n/en.ts';
import { zhCN } from '../src/services/i18n/zh-CN.ts';

const errors = [];

function walk(enNode, zhNode, path) {
  if (enNode === null || typeof enNode !== 'object' || Array.isArray(enNode)) {
    if (typeof enNode !== typeof zhNode) {
      errors.push(`${path}: 类型不一致 en=${typeof enNode} zh=${typeof zhNode}`);
    }
    return;
  }
  if (zhNode === null || typeof zhNode !== 'object' || Array.isArray(zhNode)) {
    errors.push(`${path}: zh 不是同构对象`);
    return;
  }
  const enKeys = Object.keys(enNode);
  const zhKeys = Object.keys(zhNode);
  for (const key of new Set([...enKeys, ...zhKeys])) {
    const keyPath = path ? `${path}.${key}` : key;
    if (!/^[\x00-\x7F]+$/.test(key)) {
      errors.push(`${keyPath}: key 含非 ASCII 字符（key 不允许翻译）`);
    }
    if (!(key in enNode)) errors.push(`${keyPath}: en 缺 key`);
    if (!(key in zhNode)) errors.push(`${keyPath}: zh 缺 key`);
    if (key in enNode && key in zhNode) walk(enNode[key], zhNode[key], keyPath);
  }
}

walk(en, zhCN, '');

if (errors.length > 0) {
  console.error(errors.join('\n'));
  process.exit(1);
}
console.log('verify-i18n: OK');
