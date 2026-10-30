import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { ProjectTreeContextMenu } from './ProjectTreeContextMenu';

describe('ProjectTreeContextMenu batch delete', () => {
  it('passes the whole multi-selection to onDelete', () => {
    const onDelete = vi.fn();
    render(
      <ProjectTreeContextMenu
        menu={{
          x: 10,
          y: 10,
          path: '/repo/a.ts',
          name: 'a.ts',
          isDir: false,
          selectedPaths: ['/repo/a.ts', '/repo/b.ts']
        }}
        hasClipboard={false}
        onClose={() => {}}
        onNewFile={() => {}}
        onNewDirectory={() => {}}
        onCut={() => {}}
        onCopy={() => {}}
        onPaste={() => {}}
        onCopyPath={() => {}}
        onRename={() => {}}
        onDelete={onDelete}
        onReveal={() => {}}
        onOpenTerminal={() => {}}
        onAddToVcs={() => {}}
        onRollback={() => {}}
        onExpandAll={() => {}}
        onCollapseAll={() => {}}
        onReload={() => {}}
        onShowDiff={() => {}}
        onShowHistory={() => {}}
        onAnnotate={() => {}}
        onCompareWith={() => {}}
        repoRoot="/repo"
      />
    );

    fireEvent.click(screen.getByRole('button', { name: /Delete 2 Items/i }));
    expect(onDelete).toHaveBeenCalledTimes(1);
    expect(onDelete).toHaveBeenCalledWith(
      '/repo/a.ts',
      'a.ts',
      false,
      ['/repo/a.ts', '/repo/b.ts']
    );
  });
});
