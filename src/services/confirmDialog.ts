/**
 * 全局确认弹窗服务
 *
 * 替代 macOS 原生 NSAlert（@tauri-apps/plugin-dialog confirm）。
 * 交互与视觉参考常见 IDE 习惯，LazeCode 自有实现：
 * - 左侧圆形问号徽标（主题强调色底 + 纯白 ?）
 * - 标题 + 正文两段式结构
 * - 右下角操作按钮：Cancel 灰底微边框 + Action 强调色底白字（动词贴合动作）
 * - 键盘支持：Enter 确认，Escape 取消，焦点自动入框
 */

export interface ConfirmOptions {
  title?: string;
  kind?: 'info' | 'warning' | 'error' | 'question';
  okLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
}

export interface ConfirmRequest extends ConfirmOptions {
  id: string;
  message: string;
}

type ConfirmHandler = (req: ConfirmRequest) => Promise<boolean>;

let currentHandler: ConfirmHandler | null = null;

export function registerConfirmHandler(handler: ConfirmHandler | null): void {
  currentHandler = handler;
}

/**
 * 弹出全局确认弹窗（替代 dialogConfirm）
 */
export function appConfirm(message: string, options?: ConfirmOptions | string): Promise<boolean> {
  const opts: ConfirmOptions = typeof options === 'string' ? { title: options } : (options ?? {});
  if (currentHandler) {
    return currentHandler({
      id: Math.random().toString(36).slice(2),
      message,
      ...opts,
    });
  }
  return Promise.resolve(false);
}

/**
 * 从标题与正文中智能推导动作动词（参考常见 IDE 的确认文案习惯）
 * 例：
 * "Abort Cherry-Pick" -> "Abort"
 * "Delete Branch" -> "Delete"
 * "Rollback Changes" -> "Rollback"
 * "放弃拣选" -> "放弃"
 * "删除分支" -> "删除"
 */
export function deriveActionVerb(title?: string, message?: string): string {
  const t = (title || '').toLowerCase();
  const m = (message || '').toLowerCase();
  const combined = t + ' ' + m;
  const isZh = /[\u4e00-\u9fa5]/.test(title || '') || /[\u4e00-\u9fa5]/.test(message || '');

  if (combined.includes('abort') || combined.includes('放弃') || combined.includes('中止')) {
    return isZh ? '放弃' : 'Abort';
  }
  if (
    combined.includes('delete') ||
    combined.includes('trash') ||
    combined.includes('drop') ||
    combined.includes('remove') ||
    combined.includes('删除') ||
    combined.includes('丢弃') ||
    combined.includes('移除')
  ) {
    return isZh ? '删除' : 'Delete';
  }
  if (combined.includes('rollback') || combined.includes('回滚')) {
    return isZh ? '回滚' : 'Rollback';
  }
  if (combined.includes('revert') || combined.includes('还原')) {
    return isZh ? '还原' : 'Revert';
  }
  if (combined.includes('undo') || combined.includes('撤销')) {
    return isZh ? '撤销' : 'Undo';
  }
  if (combined.includes('checkout') || combined.includes('检出')) {
    return isZh ? '检出' : 'Checkout';
  }
  if (combined.includes('merge') || combined.includes('合并')) {
    return isZh ? '合并' : 'Merge';
  }
  if (combined.includes('rebase') || combined.includes('变基')) {
    return isZh ? '变基' : 'Rebase';
  }
  if (combined.includes('save') || combined.includes('保存')) {
    return isZh ? '保存' : 'Save';
  }
  if (combined.includes('discard') || combined.includes('不保存')) {
    return isZh ? '不保存' : 'Discard';
  }
  return isZh ? '确定' : 'OK';
}
