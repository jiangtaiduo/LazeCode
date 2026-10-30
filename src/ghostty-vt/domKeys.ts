// DOM KeyboardEvent → 原生键盘编码器输入
//
// 只做「翻译」：把浏览器的 KeyboardEvent 转成引擎要的
// (key, mods, action, utf8) 四元组，再由 Rust 侧编码成字节。
// 编码本身不在这里做 —— kitty 协议标志、光标键应用模式等状态
// 只有终端自己知道，前端复刻必然失步。
//
// 使用 `code` 而非 `key` 定位物理键，避免受布局与 IME 影响。

import { GHOSTTY_KEY_VALUES } from './keyValues.generated';

/** `GhosttyMods`（取自 key/event.h 的位定义） */
export const MODS = {
  SHIFT: 1 << 0,
  CTRL: 1 << 1,
  ALT: 1 << 2,
  SUPER: 1 << 3,
  CAPS_LOCK: 1 << 4,
  NUM_LOCK: 1 << 5
} as const;

/** `GhosttyKeyAction`（取自原生引擎清单） */
export const KEY_ACTION = {
  RELEASE: 0,
  PRESS: 1,
  REPEAT: 2
} as const;

/**
 * 传给原生编码器的一次按键。
 *
 * 字段名与 Rust 侧 `vt::KeyInput` 的 serde 映射严格对应（camelCase）。
 */
export interface KeyInput {
  /** `GhosttyKey` */
  key: number;
  /** `GhosttyMods` 位域 */
  mods: number;
  /** `GhosttyKeyAction` */
  action: number;
  /** 该键产生的文本，控制键可为空 */
  utf8?: string;
  /** 未受 Shift 影响的码点，kitty 协议需要 */
  unshiftedCodepoint?: number;
  composing?: boolean;
}

/**
 * `GhosttyKey` 键值。
 *
 * 数值不再手写：由 `scripts/gen-key-table.mjs` 从 vendored
 * `key/event.h` 的 `GhosttyKey` 枚举生成（`npm run gen:key-table`），
 * `verify:key-table` 与 `verify:wire` 双重防止漂移。
 *
 * 历史教训：本项目曾因 MINUS/PERIOD/QUOTE/SEMICOLON/SLASH 各偏 1，
 * 导致 `-` `.` `'` `;` `/` 发出错误的键；整组 F2..F24 / JIS 键缺失时
 * 症状同样是「按键无反应」，很难与键盘事件丢失区分。
 */
const GHOSTTY_KEY = {
  ...GHOSTTY_KEY_VALUES
} as const;

/// DOM `KeyboardEvent.code` -> 引擎键码的静态映射。
///
/// 提升到模块级：原先每次按键都在函数体内新建这个 34 项对象字面量，
/// 键盘事件是最高频的输入路径，没必要每次分配 + 重新哈希。
/// 表在模块加载时求值一次，之后只读。
const CODE_TO_KEY: Record<string, number> = {
  Backquote: GHOSTTY_KEY.BACKQUOTE,
  Backslash: GHOSTTY_KEY.BACKSLASH,
  BracketLeft: GHOSTTY_KEY.BRACKET_LEFT,
  BracketRight: GHOSTTY_KEY.BRACKET_RIGHT,
  Comma: GHOSTTY_KEY.COMMA,
  Equal: GHOSTTY_KEY.EQUAL,
  Minus: GHOSTTY_KEY.MINUS,
  Period: GHOSTTY_KEY.PERIOD,
  Quote: GHOSTTY_KEY.QUOTE,
  Semicolon: GHOSTTY_KEY.SEMICOLON,
  Slash: GHOSTTY_KEY.SLASH,
  Backspace: GHOSTTY_KEY.BACKSPACE,
  CapsLock: GHOSTTY_KEY.CAPS_LOCK,
  Delete: GHOSTTY_KEY.DELETE,
  End: GHOSTTY_KEY.END,
  Enter: GHOSTTY_KEY.ENTER,
  NumpadEnter: GHOSTTY_KEY.NUMPAD_ENTER,
  IntlBackslash: GHOSTTY_KEY.INTL_BACKSLASH,
  IntlRo: GHOSTTY_KEY.INTL_RO,
  IntlYen: GHOSTTY_KEY.INTL_YEN,
  Escape: GHOSTTY_KEY.ESCAPE,
  Home: GHOSTTY_KEY.HOME,
  Insert: GHOSTTY_KEY.INSERT,
  PageDown: GHOSTTY_KEY.PAGE_DOWN,
  PageUp: GHOSTTY_KEY.PAGE_UP,
  ArrowDown: GHOSTTY_KEY.ARROW_DOWN,
  ArrowLeft: GHOSTTY_KEY.ARROW_LEFT,
  ArrowRight: GHOSTTY_KEY.ARROW_RIGHT,
  ArrowUp: GHOSTTY_KEY.ARROW_UP,
  Space: GHOSTTY_KEY.SPACE,
  Tab: GHOSTTY_KEY.TAB,
  Numpad0: GHOSTTY_KEY.NUMPAD_0,
  Numpad1: GHOSTTY_KEY.NUMPAD_1,
  Numpad2: GHOSTTY_KEY.NUMPAD_2,
  Numpad3: GHOSTTY_KEY.NUMPAD_3,
  Numpad4: GHOSTTY_KEY.NUMPAD_4,
  Numpad5: GHOSTTY_KEY.NUMPAD_5,
  Numpad6: GHOSTTY_KEY.NUMPAD_6,
  Numpad7: GHOSTTY_KEY.NUMPAD_7,
  Numpad8: GHOSTTY_KEY.NUMPAD_8,
  Numpad9: GHOSTTY_KEY.NUMPAD_9,
  NumpadAdd: GHOSTTY_KEY.NUMPAD_ADD,
  NumpadSubtract: GHOSTTY_KEY.NUMPAD_SUBTRACT,
  NumpadMultiply: GHOSTTY_KEY.NUMPAD_MULTIPLY,
  NumpadDivide: GHOSTTY_KEY.NUMPAD_DIVIDE,
  NumpadDecimal: GHOSTTY_KEY.NUMPAD_DECIMAL,
  NumpadEqual: GHOSTTY_KEY.NUMPAD_EQUAL,
  NumpadComma: GHOSTTY_KEY.NUMPAD_COMMA,
  NumpadParenLeft: GHOSTTY_KEY.NUMPAD_PAREN_LEFT,
  NumpadParenRight: GHOSTTY_KEY.NUMPAD_PAREN_RIGHT,
  ShiftLeft: GHOSTTY_KEY.SHIFT_LEFT,
  ShiftRight: GHOSTTY_KEY.SHIFT_RIGHT,
  ControlLeft: GHOSTTY_KEY.CONTROL_LEFT,
  ControlRight: GHOSTTY_KEY.CONTROL_RIGHT,
  AltLeft: GHOSTTY_KEY.ALT_LEFT,
  AltRight: GHOSTTY_KEY.ALT_RIGHT,
  MetaLeft: GHOSTTY_KEY.META_LEFT,
  MetaRight: GHOSTTY_KEY.META_RIGHT,
  ContextMenu: GHOSTTY_KEY.CONTEXT_MENU,
  NumLock: GHOSTTY_KEY.NUM_LOCK,
  Convert: GHOSTTY_KEY.CONVERT,
  KanaMode: GHOSTTY_KEY.KANA_MODE,
  NonConvert: GHOSTTY_KEY.NON_CONVERT,
  PrintScreen: GHOSTTY_KEY.PRINT_SCREEN,
  ScrollLock: GHOSTTY_KEY.SCROLL_LOCK
};

function codeToGhosttyKey(code: string): number | null {
  // 字母：DOM 的 code 是 "KeyA".."KeyZ"（4 字符），A=20 起连续
  const letter = /^Key([A-Z])$/.exec(code);
  if (letter) {
    return GHOSTTY_KEY.A + (letter[1].charCodeAt(0) - 65);
  }

  // 数字 Digit0-9：DIGIT_0=6 起连续
  if (code.startsWith('Digit')) {
    const d = Number(code.slice(5));
    if (d >= 0 && d <= 9) return GHOSTTY_KEY.DIGIT_0 + d;
  }

  // 功能键 F1-F24：F1=121 起连续
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) {
    return GHOSTTY_KEY.F1 + (Number(code.slice(1)) - 1);
  }

  return CODE_TO_KEY[code] ?? null;
}

function toMods(e: KeyboardEvent): number {
  let mods = 0;
  if (e.shiftKey) mods |= MODS.SHIFT;
  if (e.ctrlKey) mods |= MODS.CTRL;
  if (e.altKey) mods |= MODS.ALT;
  if (e.metaKey) mods |= MODS.SUPER;
  // CapsLock / NumLock 只有 getModifierState 能拿到（e.capsLock 不存在）。
  // 声明了位却从不置位会让引擎无法区分「小键盘 8」与「上方向键」，
  // 也会让 kitty 协议的修饰键上报缺位。
  if (e.getModifierState?.('CapsLock')) mods |= MODS.CAPS_LOCK;
  if (e.getModifierState?.('NumLock')) mods |= MODS.NUM_LOCK;
  return mods;
}

/**
 * 将 DOM 事件转为引擎输入。
 *
 * 可打印字符同时携带 `utf8`，让引擎在 `modifyOtherKeys` / Kitty 协议下
 * 选择正确的编码形式；控制键只依赖 key + mods。
 */
export function domKeyToInput(e: KeyboardEvent, action?: number): KeyInput | null {
  const key = codeToGhosttyKey(e.code);
  if (key === null) return null;

  const resolvedAction = action ?? (e.repeat ? KEY_ACTION.REPEAT : KEY_ACTION.PRESS);
  const mods = toMods(e);

  // Ctrl/Alt 组合通常由引擎按控制序列编码，不附加文本；
  // 纯可打印输入才带上 utf8。
  // 修饰键（CapsLock/Shift/Ctrl/Alt/Meta）的 e.key 虽然有值（如 "CapsLock"），
  // 但它们不应产生文本——否则会被 PTY 当作字面量输入。
  const isControlCombo = (e.ctrlKey || e.metaKey) && !e.altKey;
  const isModifier =
    e.key === 'CapsLock' || e.key === 'Shift' || e.key === 'Control' || e.key === 'Alt' || e.key === 'Meta';
  const utf8 =
    !isControlCombo && !isModifier && e.key.length >= 1 && !/^(Enter|Tab|Escape|Backspace|Delete|Insert|Home|End|PageUp|PageDown|ArrowUp|ArrowDown|ArrowLeft|ArrowRight|F\d+|NumLock|ScrollLock|PrintScreen)$/i.test(e.key)
      ? e.key
      : undefined;

  return {
    key,
    mods,
    action: resolvedAction,
    utf8,
    // kitty 协议用未受 Shift 影响的码点区分 Shift+数字 等组合
    unshiftedCodepoint: e.key.length === 1 ? e.key.toLowerCase().codePointAt(0) ?? 0 : 0,
    composing: e.isComposing
  };
}
