// 编辑器右键菜单 · 复用全局 CtxMenuShell。
//
// Monaco 的默认右键是英文 Web 风格菜单，与本项目的中文 PyCharm 风格割裂，
// 且不含 Git 语义。因此在 main.tsx 把 `.monaco-editor` 移出原生菜单白名单，
// 改由这里提供同款菜单。
//
// git 相关项复用 `ProjectTreeContextMenu` 的语义（同样由 App.tsx 提供回调），
// 编辑语义项走 Monaco 的 action id —— 只触发不重写。

import React from 'react';
import { CtxMenuShell } from './CtxMenuShell';
import { t } from '../services/i18n';

export interface EditorMenuState {
  x: number;
  y: number;
  /** 右键时是否命中了可重命名的符号（有语言服务时为 true） */
  hasSymbol: boolean;
}

interface EditorContextMenuProps {
  menu: EditorMenuState | null;
  onClose: () => void;
  /** Monaco 命令执行器：由 EditorView 注入 editor.getAction(id)?.run() */
  runAction: (actionId: string) => void;
  onCopy?: () => void;
  onShowHistory?: () => void;
  onAnnotate?: () => void;
  /** 仓库外 / 非 git 仓库：Git 分组整组隐藏 */
  showGit: boolean;
}

/** 菜单行：纯文本 + 可选 danger / disabled。 */
const Row: React.FC<{
  label: string;
  onClick: () => void;
  disabled?: boolean;
  danger?: boolean;
}> = ({ label, onClick, disabled, danger }) => (
  <button
    className={`ctx-item${danger ? ' ctx-danger' : ''}`}
    disabled={disabled}
    onClick={() => {
      onClick();
    }}
  >
    <span className="ctx-label">{label}</span>
  </button>
);

const CtxSep: React.FC = () => <div className="ctx-sep" />;

export const EditorContextMenu: React.FC<EditorContextMenuProps> = ({
  menu,
  onClose,
  runAction,
  onCopy,
  onShowHistory,
  onAnnotate,
  showGit,
}) => {
  if (!menu) return null;
  const { x, y, hasSymbol } = menu;

  const act = (fn: () => void) => () => {
    onClose();
    fn();
  };

  return (
    <CtxMenuShell
      className="editor-ctx-menu"
      style={{ top: y, left: x }}
      onClose={onClose}
    >
      <Row label={t().editorMenu.goToDefinition} onClick={act(() => runAction('editor.action.revealDefinition'))} />
      <Row label={t().editorMenu.findReferences} onClick={act(() => runAction('editor.action.referenceSearch.trigger'))} />
      <Row label={t().editorMenu.renameSymbol} onClick={act(() => runAction('editor.action.rename'))} disabled={!hasSymbol} />
      <CtxSep />
      <Row label={t().menu.cut} onClick={act(() => runAction('editor.action.clipboardCutAction'))} />
      <Row label={t().menu.copy} onClick={act(() => (onCopy ? onCopy() : runAction('editor.action.clipboardCopyAction')))} />
      <Row label={t().menu.paste} onClick={act(() => runAction('editor.action.clipboardPasteAction'))} />
      <CtxSep />
      <Row label={t().editorMenu.formatDocument} onClick={act(() => runAction('editor.action.formatDocument'))} />
      <Row label={t().menu.goToLine} onClick={act(() => runAction('editor.action.gotoLine'))} />
      {showGit && (
        <>
          <CtxSep />
          <Row label={t().treeMenu.showHistory} onClick={act(() => onShowHistory?.())} />
          <Row label={t().treeMenu.annotateBlame} onClick={act(() => onAnnotate?.())} />
        </>
      )}
    </CtxMenuShell>
  );
};
