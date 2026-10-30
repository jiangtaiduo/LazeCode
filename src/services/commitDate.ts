// 提交/贮藏时间的 PyCharm 风格格式化。
//
// 后端 git log 统一输出 unix 秒（%ct），显示形态与 PyCharm 一致：
//   今天   → Today 17:23
//   昨天   → Yesterday 19:35
//   更早   → 2026/9/18 15:23
// 非纯数字入参（旧数据 / 其它命令的相对时间串）原样返回，避免出现 Invalid Date。

/** 本地时区当天零点 */
function startOfDay(d: Date): number {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

/** unix 秒 → PyCharm 风格时间标签；入参不是秒级时间戳时原样返回 */
export function formatCommitDate(raw: string | number | undefined | null): string {
  if (raw === null || raw === undefined || raw === '') return '';
  const text = typeof raw === 'string' ? raw : String(raw);
  if (!/^\d{9,}$/.test(text.trim())) return text;
  const ms = Number(text) * 1000;
  const d = new Date(ms);
  if (Number.isNaN(d.getTime())) return text;

  const hh = String(d.getHours()).padStart(2, '0');
  const mm = String(d.getMinutes()).padStart(2, '0');
  const time = `${hh}:${mm}`;

  const today = startOfDay(new Date());
  const day = startOfDay(d);
  const diffDays = Math.round((today - day) / 86400000);
  if (diffDays === 0) return `Today ${time}`;
  if (diffDays === 1) return `Yesterday ${time}`;
  if (diffDays < 0) return `${d.getFullYear()}/${d.getMonth() + 1}/${d.getDate()} ${time}`;
  return `${d.getFullYear()}/${d.getMonth() + 1}/${d.getDate()} ${time}`;
}
