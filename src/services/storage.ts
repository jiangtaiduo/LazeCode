// localStorage 安全读写（从 App.tsx 迁出，行为不变）。
//
// 两个函数解决同一个问题：隐私模式/数据损坏不能把应用带崩。
// - 读（面板尺寸、JSON 数组）：异常或畸形时回退默认值，避免挂载即白屏。
// - 写：QuotaExceededError 沿同步调用链上冒会中断主流程
//   （曾实测：saveRecentWorkspace 的写盘失败让整个「打开项目」中断且无提示），
//   持久化是增强，任何失败都不该阻断主流程。

/** 读取持久化的面板尺寸：异常/畸形时安全回退，避免挂载即白屏 */
export const readPanelSize = (key: string, fallback: number, min: number, max: number): number => {
  try {
    const saved = Number(localStorage.getItem(key));
    return Number.isFinite(saved) && saved > 0 ? Math.max(min, Math.min(max, saved)) : fallback;
  } catch {
    return fallback;
  }
};

/**
 * 写入 localStorage，失败（隐私模式禁用 / 配额耗尽）只吞掉不影响功能。
 */
export const writeStorage = (key: string, value: string): void => {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* 隐私模式 / 配额耗尽：本次不落盘，内存中的状态仍然有效 */
  }
};

/**
 * 读取「字符串数组」形态的持久化记录（收藏分支、提交信息历史、最近项目）。
 *
 * 必须是数组且元素全是字符串才返回有效内容：损坏的 JSON（对象/null/字符串）
 * 若直接透出，下游的 `.filter`/展开会抛 TypeError → ErrorBoundary 白屏。
 * 坏数据清掉（下次写入覆盖），读失败一律给空数组。
 */
export const readStringArray = (key: string): string[] => {
  try {
    const raw = localStorage.getItem(key);
    const parsed: unknown = raw ? JSON.parse(raw) : [];
    return Array.isArray(parsed) ? parsed.filter((x): x is string => typeof x === 'string') : [];
  } catch {
    return [];
  }
};
