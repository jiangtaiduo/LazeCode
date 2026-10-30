import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { CommitDrawer } from './CommitDrawer';
import type { GitChangedFile } from '../types';

vi.mock('../services/tauriBridge', () => ({
  invokeCommand: vi.fn(async () => [])
}));

const files: GitChangedFile[] = [
  { path: 'src/a.ts', status: 'M', isStaged: false, isUnstaged: true, additions: 1, deletions: 0 },
  { path: 'src/b.ts', status: 'M', isStaged: false, isUnstaged: true, additions: 1, deletions: 0 }
];

function renderDrawer(overrides: Record<string, unknown> = {}) {
  return render(
    <CommitDrawer
      changedFiles={files}
      selectedFile=""
      onSelectFileForDiff={vi.fn()}
      onCommit={vi.fn()}
      onCommitAndPush={vi.fn()}
      repoPath="/repo"
      {...overrides}
    />
  );
}

describe('CommitDrawer change list', () => {
  beforeEach(() => {
    window.localStorage.clear();
  });

  it('starts with every changed file unchecked', () => {
    renderDrawer();
    const boxes = Array.from(
      document.querySelectorAll<HTMLInputElement>('.change-file-row input[type="checkbox"]')
    );
    expect(boxes).toHaveLength(2);
    expect(boxes.every(box => !box.checked)).toBe(true);
  });

  it('only opens the diff on double-click unless preview is enabled', () => {
    const onSelectFileForDiff = vi.fn();
    renderDrawer({ onSelectFileForDiff });
    const rowA = document.querySelector<HTMLElement>('.change-file-row[data-path="src/a.ts"]')!;

    fireEvent.click(rowA);
    expect(onSelectFileForDiff).not.toHaveBeenCalled();
    fireEvent.doubleClick(rowA);
    expect(onSelectFileForDiff).toHaveBeenCalledWith('src/a.ts', false);
  });

  it('supports additive selection and runs the menu on the whole selection', () => {
    const onSelectFileForDiff = vi.fn();
    const onCommitPaths = vi.fn();
    renderDrawer({ onSelectFileForDiff, onCommitPaths });

    const rowA = document.querySelector<HTMLElement>('.change-file-row[data-path="src/a.ts"]')!;
    const rowB = document.querySelector<HTMLElement>('.change-file-row[data-path="src/b.ts"]')!;
    fireEvent.click(rowA, { metaKey: true });
    fireEvent.click(rowB, { metaKey: true });

    expect(document.querySelectorAll('.change-file-row.selected')).toHaveLength(2);
    expect(onSelectFileForDiff).not.toHaveBeenCalled();

    fireEvent.contextMenu(rowB, { clientX: 120, clientY: 120 });
    fireEvent.click(screen.getByText('Commit Files...'));
    expect(onCommitPaths).toHaveBeenCalledWith(['src/a.ts', 'src/b.ts']);
  });
});
