#!/usr/bin/env node
// 遥测与历史存储验证（运行: npm run verify:telemetry）。
//
// 覆盖：事件名规则、opt-out、命令埋点分类、高频跳过、历史头插/去重/
// 截断/切换/删除、旧 key 认领迁移、单值与对象数组存取。
// 用法与其它 verify 脚本一致。

import {
  COMMIT_MESSAGES,
  FAVORITE_BRANCHES,
  RECENT_WORKSPACES,
  LAST_WORKSPACE,
  TELEMETRY_EVENTS,
  loadHistory,
  pushHistory,
  removeFromHistory,
  toggleHistory,
  clearHistory,
  loadLastWorkspace,
  saveLastWorkspace,
  clearLastWorkspace,
  loadJsonArray,
  saveJsonArray,
  setHistoryBackend,
  memoryBackend,
} from '../src/services/historyStore.ts';
import {
  isTelemetryEnabled,
  setTelemetryEnabled,
  isValidEventName,
  trackEvent,
  trackCommand,
  recentEvents,
  flushTelemetry,
  configureTelemetryStore,
  TELEMETRY_BUFFER_MAX,
} from '../src/services/telemetry.ts';

// Node has no localStorage; provide a tiny in-memory implementation so the
// opt-in gate can be exercised. Diagnostics must be off by default.
const localStore = new Map();
globalThis.localStorage = {
  getItem: key => (localStore.has(key) ? localStore.get(key) : null),
  setItem: (key, value) => localStore.set(key, String(value)),
  removeItem: key => localStore.delete(key),
};
if (isTelemetryEnabled()) {
  console.error('FAIL  本地诊断默认必须是关闭的');
  process.exitCode = 1;
}
setTelemetryEnabled(true);
if (!isTelemetryEnabled()) {
  console.error('FAIL  显式开启后 isTelemetryEnabled 应为 true');
  process.exitCode = 1;
}

setHistoryBackend(memoryBackend());
// telemetry 接内存（与 historyStore 同后端，行为一致）
configureTelemetryStore({
  load: () => loadJsonArray(TELEMETRY_EVENTS),
  save: (events) => saveJsonArray(TELEMETRY_EVENTS, events),
});

// 缓冲上限必须与 historyStore.TELEMETRY_EVENTS.max 同值（两处常量，见 telemetry.ts）
let failures = 0;
const assert = (cond, msg, extra) => {
  if (cond) console.log(`  ok  ${msg}`);
  else {
    failures += 1;
    console.error(`FAIL  ${msg}${extra !== undefined ? `（实际 ${JSON.stringify(extra)}）` : ''}`);
  }
};
assert(
  TELEMETRY_BUFFER_MAX === TELEMETRY_EVENTS.max,
  'telemetry 缓冲上限与 historyStore 队列上限一致',
  [TELEMETRY_BUFFER_MAX, TELEMETRY_EVENTS.max],
);

// ---- 事件名规则 ----
assert(isValidEventName('Command Executed'), '标准名通过');
assert(isValidEventName('Project Opened'), '标准名通过 2');
assert(isValidEventName('Settings Imported'), '标准名通过 3');
assert(!isValidEventName('command Executed'), '小写开头拒绝');
assert(!isValidEventName('Opened'), '单词拒绝');
assert(!isValidEventName(''), '空串拒绝');
assert(!isValidEventName('A B C'), '三词拒绝');
assert(!isValidEventName('Command  Executed'), '双空格拒绝');

// ---- trackEvent ----
{
  const e = trackEvent('Project Opened');
  assert(e !== null && e.name === 'Project Opened' && typeof e.ts === 'number', '合法事件入队');
  const bad = trackEvent('opened!');
  assert(bad === null, '非法名丢弃');
}

// ---- trackCommand：分类来自目录，高频跳过 ----
{
  const before = recentEvents(1000).length;
  trackCommand('git_status', 12.7, true, 'git');
  trackCommand('read_file_content', 3.2, false, 'files');
  trackCommand('vt_snapshot', 1.1, true, 'terminal');
  trackCommand('vt_snapshot_diff', 0.9, true, 'terminal');
  const after = recentEvents(1000);
  assert(after.length === before + 2, '高频命令不记，只进 2 条', after.length - before);
  const git = after[after.length - 2];
  assert(git.props.command === 'git_status' && git.props.category === 'git', 'git 分类', git.props);
  assert(git.props.ms === 13 && git.props.ok === true, '耗时取整 + 成败', git.props);
  const file = after[after.length - 1];
  assert(file.props.category === 'files' && file.props.ok === false, '失败标记', file.props);
}

// ---- 历史：头插/去重/截断 ----
{
  const spec = { key: 't-cap', max: 3 };
  assert(JSON.stringify(pushHistory(spec, 'a')) === '["a"]', '首条');
  assert(JSON.stringify(pushHistory(spec, 'b')) === '["b","a"]', '头插');
  assert(JSON.stringify(pushHistory(spec, 'a')) === '["a","b"]', '去重顶');
  pushHistory(spec, 'c');
  const capped = pushHistory(spec, 'd');
  assert(JSON.stringify(capped) === '["d","c","a"]', '截断到 max', capped);
}

// ---- 切换/删除/清空 ----
{
  const spec = { key: 't-toggle', max: 50 };
  const r1 = toggleHistory(spec, 'main');
  assert(r1.added === true && JSON.stringify(r1.list) === '["main"]', '新增');
  const r2 = toggleHistory(spec, 'main');
  assert(r2.added === false && r2.list.length === 0, '再切即删');
  pushHistory(spec, 'a');
  pushHistory(spec, 'b');
  assert(JSON.stringify(removeFromHistory(spec, 'a')) === '["b"]', '删除单条');
  clearHistory(spec);
  assert(loadHistory(spec).length === 0, '清空');
}

// ---- 旧 key 认领迁移（搬家：写新位后删旧位） ----
{
  const mem = memoryBackend();
  setHistoryBackend(mem);
  mem.set('ide_recent_commit_messages', '["m1","m2"]');
  const loaded = loadHistory(COMMIT_MESSAGES);
  assert(JSON.stringify(loaded) === '["m1","m2"]', '旧数据被认领', loaded);
  assert(mem.get('ide_recent_commit_messages') === null, '旧位已删');
  assert(mem.get('lazecode.history.v1.commitMessages') === '["m1","m2"]', '新位落盘');
  // Node has no localStorage; provide a tiny in-memory implementation so the
// opt-in gate can be exercised. Diagnostics must be off by default.
const localStore = new Map();
globalThis.localStorage = {
  getItem: key => (localStore.has(key) ? localStore.get(key) : null),
  setItem: (key, value) => localStore.set(key, String(value)),
  removeItem: key => localStore.delete(key),
};
if (isTelemetryEnabled()) {
  console.error('FAIL  本地诊断默认必须是关闭的');
  process.exitCode = 1;
}
setTelemetryEnabled(true);
if (!isTelemetryEnabled()) {
  console.error('FAIL  显式开启后 isTelemetryEnabled 应为 true');
  process.exitCode = 1;
}

setHistoryBackend(memoryBackend());
}

// ---- 坏数据不崩 ----
{
  const mem = memoryBackend();
  setHistoryBackend(mem);
  mem.set('lazecode.history.v1.commitMessages', '{oops');
  assert(loadHistory(COMMIT_MESSAGES).length === 0, '坏 JSON 给空');
  mem.set('lazecode.history.v1.commitMessages', '{"a":1}');
  assert(loadHistory(COMMIT_MESSAGES).length === 0, '非数组给空');
  // Node has no localStorage; provide a tiny in-memory implementation so the
// opt-in gate can be exercised. Diagnostics must be off by default.
const localStore = new Map();
globalThis.localStorage = {
  getItem: key => (localStore.has(key) ? localStore.get(key) : null),
  setItem: (key, value) => localStore.set(key, String(value)),
  removeItem: key => localStore.delete(key),
};
if (isTelemetryEnabled()) {
  console.error('FAIL  本地诊断默认必须是关闭的');
  process.exitCode = 1;
}
setTelemetryEnabled(true);
if (!isTelemetryEnabled()) {
  console.error('FAIL  显式开启后 isTelemetryEnabled 应为 true');
  process.exitCode = 1;
}

setHistoryBackend(memoryBackend());
}

// ---- 单值与对象数组 ----
{
  saveLastWorkspace('/tmp/proj');
  assert(loadLastWorkspace() === '/tmp/proj', '单值存取');
  clearLastWorkspace();
  assert(loadLastWorkspace() === null, '单值清空');
  saveJsonArray(TELEMETRY_EVENTS, [{ ts: 1, name: 'Command Executed', props: {} }]);
  const arr = loadJsonArray(TELEMETRY_EVENTS);
  assert(arr.length === 1 && arr[0].name === 'Command Executed', '对象数组存取');
}

// ---- 落盘节流：25 条刷一次，flush 兜底（增量断言，与前文条数无关） ----
{
  flushTelemetry();
  const disk0 = loadJsonArray(TELEMETRY_EVENTS).length;
  for (let i = 0; i < 25; i += 1) trackEvent('Project Opened');
  assert(loadJsonArray(TELEMETRY_EVENTS).length === disk0 + 25, '攒 25 条自动刷盘');
  trackEvent('Project Opened');
  flushTelemetry();
  assert(loadJsonArray(TELEMETRY_EVENTS).length === disk0 + 26, 'flush 兜底余量');
}

// ---- 双上限：条数 + 字节预算（Zed #60134 的教训：单条大也能撑爆存储） ----
{
  const mem = memoryBackend();
  setHistoryBackend(mem);
  // 单条 100KB 推 10 条：条数未超但体积超 64KB 预算 → 从最旧淘汰，只留最新
  const big = 'x'.repeat(100 * 1024);
  for (let i = 0; i < 10; i += 1) pushHistory(COMMIT_MESSAGES, `${big}#${i}`);
  const kept = loadHistory(COMMIT_MESSAGES);
  assert(kept.length === 1 && kept[0].endsWith('#9'), '字节超预算时只留最新一条', kept.length);
  assert(
    (mem.get('lazecode.history.v1.commitMessages') ?? '').length <= (COMMIT_MESSAGES.maxBytes ?? 0) + big.length,
    '落盘体积被预算封顶'
  );
  // 单条就超预算：至少保留最新 1 条（不给空），而不是全丢
  pushHistory(FAVORITE_BRANCHES, 'y'.repeat(100 * 1024));
  assert(loadHistory(FAVORITE_BRANCHES).length === 1, '单条超预算仍保留最新');
  // 单值超预算：拒绝落盘，保留旧值
  saveLastWorkspace('/tmp/ok');
  saveLastWorkspace('/tmp/' + 'z'.repeat(8 * 1024));
  assert(loadLastWorkspace() === '/tmp/ok', '单值超预算拒绝覆盖');
  // 对象数组同样收敛（遥测队列被误塞大 props 时）
  saveJsonArray(TELEMETRY_EVENTS, [{ ts: 1, name: 'Command Executed', props: { blob: big } }]);
  const arr = loadJsonArray(TELEMETRY_EVENTS);
  assert(arr.length === 1, '对象数组单条超预算仍保留最新');
  // Node has no localStorage; provide a tiny in-memory implementation so the
// opt-in gate can be exercised. Diagnostics must be off by default.
const localStore = new Map();
globalThis.localStorage = {
  getItem: key => (localStore.has(key) ? localStore.get(key) : null),
  setItem: (key, value) => localStore.set(key, String(value)),
  removeItem: key => localStore.delete(key),
};
if (isTelemetryEnabled()) {
  console.error('FAIL  本地诊断默认必须是关闭的');
  process.exitCode = 1;
}
setTelemetryEnabled(true);
if (!isTelemetryEnabled()) {
  console.error('FAIL  显式开启后 isTelemetryEnabled 应为 true');
  process.exitCode = 1;
}

setHistoryBackend(memoryBackend());
}

if (failures > 0) {
  console.error(`\n${failures} 处失败`);
  process.exit(1);
}
console.log('ok 遥测与历史存储验证通过');
