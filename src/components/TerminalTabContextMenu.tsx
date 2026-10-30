// 终端标签栏右键菜单 · 对齐 PyCharm Terminal 视图右键
//
// PyCharm 终端标签右键提供：Copy Working Directory / Close / Close Others /
// Close All / Rename。与编辑器标签栏（EditorTabBar.tsx 五种关闭变体）保持同一
// 交互口径，复用 CtxMenuShell（portal + Esc + 遮罩）统一全局右键菜单行为。

import React from 'react';
import { CtxMenuShell } from './CtxMenuShell';
import { t } from '../services/i18n';

export interface TerminalTabMenuState {
  x: number;
  y: number;
  /** 右键的标签 id；null 表示菜单关闭 */
  tabId: string | null;
}

interface TerminalTabContextMenuProps {
  menu: TerminalTabMenuState;
  onClose: () => void;
  onCloseTab: (tabId: string) => void;
  onCloseOthers: (tabId: string) => void;
  onCloseAll: () => void;
  onRename: (tabId: string) => void;
  /** 该标签会话的工作目录（终端会话的 projectKey）；为空时 Copy 项置灰 */
  cwd?: string;
  /** Copy Working Directory：由调用方写剪贴板并自行处理失败 */
  onCopyWorkingDirectory?: (cwd: string) => void;
  /** 是否还能继续分屏（单标签最多 4 个分屏） */
  canSplit?: boolean;
  /** 该标签是否有可关闭的分屏（仅多分屏时可用） */
  canClosePane?: boolean;
  onSplitRight?: (tabId: string) => void;
  onSplitDown?: (tabId: string) => void;
  onClosePane?: (tabId: string) => void;
}

const CtxSep: React.FC = () => <div className="ctx-sep" />;

const Row: React.FC<{ label: string; disabled?: boolean; onClick?: () => void }> = ({ label, disabled, onClick }) => (
  <button className="ctx-item ctx-row" disabled={disabled} onClick={onClick}>
    <span className="ctx-label">{label}</span>
  </button>
);

export const TerminalTabContextMenu: React.FC<TerminalTabContextMenuProps> = ({
  menu,
  onClose,
  onCloseTab,
  onCloseOthers,
  onCloseAll,
  onRename,
  cwd,
  onCopyWorkingDirectory,
  canSplit = false,
  canClosePane = false,
  onSplitRight,
  onSplitDown,
  onClosePane
}) => {
  if (!menu.tabId) return null;
  // 关闭后 tabId 置空；此处收窄让后续回调不必再断言
  const tabId = menu.tabId;

  // 锚点坐标直接透传给 CtxMenuShell：它挂载后测量真实尺寸并按视口做最终
  // clamp（四边留 8px），任何预留高度的魔法数都不再需要。原先的
  // innerHeight-190 / innerWidth-220 与 CSS width:150px 对不上，改菜单行数
  // 就失效，已删除。menu.x/menu.y 来自 onContextMenu 的非负 clientX/clientY。
  const top = menu.y;
  const left = menu.x;

  const act = (fn: () => void) => () => {
    onClose();
    fn();
  };

  const hasCwd = !!cwd;

  return (
    <CtxMenuShell className="terminal-tab-ctx-menu" style={{ top, left }} onClose={onClose}>
      <Row
        label={t().terminal.copyWorkingDirectory}
        disabled={!hasCwd || !onCopyWorkingDirectory}
        onClick={act(() => onCopyWorkingDirectory?.(cwd as string))}
      />
      <CtxSep />
      <Row
        label={t().terminal.splitRight}
        disabled={!canSplit || !onSplitRight}
        onClick={act(() => onSplitRight?.(tabId))}
      />
      <Row
        label={t().terminal.splitDown}
        disabled={!canSplit || !onSplitDown}
        onClick={act(() => onSplitDown?.(tabId))}
      />
      <Row
        label={t().terminal.closePane}
        disabled={!canClosePane || !onClosePane}
        onClick={act(() => onClosePane?.(tabId))}
      />
      <CtxSep />
      <Row label={t().terminal.close} onClick={act(() => onCloseTab(tabId))} />
      <CtxSep />
      <Row label={t().terminal.closeOthers} onClick={act(() => onCloseOthers(tabId))} />
      <Row label={t().terminal.closeAll} onClick={act(() => onCloseAll())} />
      <CtxSep />
      <Row label={t().terminal.rename} onClick={act(() => onRename(tabId))} />
    </CtxMenuShell>
  );
};
