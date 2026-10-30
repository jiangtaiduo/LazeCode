import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { BranchContextMenu } from './BranchContextMenu';
import type { GitBranch } from '../types';

const local = (name: string, extra: Partial<GitBranch> = {}): GitBranch => ({
  name,
  isCurrent: false,
  isRemote: false,
  ...extra
});

const remote = (name: string): GitBranch => ({ name, isCurrent: false, isRemote: true });
const tag = (name: string): GitBranch => ({ name, isCurrent: false, isRemote: false, isTag: true });

const actions = {
  onClose: vi.fn(),
  onCheckout: vi.fn(),
  onCreateBranchFrom: vi.fn(),
  onRename: vi.fn(),
  onDelete: vi.fn(),
  onCompare: vi.fn(),
  onShowDiffWithWorktree: vi.fn(),
  onUpdate: vi.fn(),
  onPush: vi.fn(),
  onMergeIntoCurrent: vi.fn(),
  onRebaseOntoCurrent: vi.fn(),
  onPullInto: vi.fn(),
  onCheckoutAndRebase: vi.fn(),
  onCompareBranches: vi.fn(),
  onShowFilesDiff: vi.fn(),
  onUpdateSelected: vi.fn(),
  onDeleteSelected: vi.fn(),
  onCheckoutTag: vi.fn(),
  onPushTag: vi.fn(),
  onDeleteTag: vi.fn()
};

const menuLabels = () =>
  screen.getAllByRole('button').map(b => b.textContent?.replace(/\s+/g, ' ').trim() ?? '');

const renderMenu = (branch: GitBranch, branches?: GitBranch[]) =>
  render(
    <BranchContextMenu
      menu={{ x: 10, y: 10, branch, branches }}
      currentBranch="main"
      {...actions}
    />
  );

describe('BranchContextMenu PyCharm parity', () => {
  it('shows the tag-only action set', () => {
    renderMenu(tag('v1.0'));
    expect(menuLabels()).toEqual([
      'Checkout',
      'Show Diff with Working Tree',
      "Merge 'v1.0' into 'main'",
      'Push to origin',
      'Delete'
    ]);
  });

  it('hides destructive/checkout items for the current local branch', () => {
    renderMenu(local('main', { isCurrent: true }));
    expect(menuLabels()).toEqual([
      "New Branch from 'main'…",
      'Show Diff with Working Tree',
      'Update',
      'Push…',
      'Rename…F2'
    ]);
  });

  it('uses plain Update/Delete labels for a non-current local branch', () => {
    renderMenu(local('feat'));
    const labels = menuLabels();
    expect(labels[0]).toBe("Checkout 'feat'");
    expect(labels).toContain('Update');
    expect(labels).toContain('Delete');
    expect(labels).not.toContain("Delete 'feat'");
  });

  it('uses the full remote menu and no rename/update', () => {
    renderMenu(remote('origin/main'));
    const labels = menuLabels();
    expect(labels[0]).toBe('Checkout');
    expect(labels).toContain("Checkout and Rebase onto 'main'");
    expect(labels).toContain("Pull into 'main' Using Merge");
    expect(labels).toContain('Delete');
    expect(labels).not.toContain('Rename…F2');
    expect(labels).not.toContain('Push…');
  });

  it('renders the four multi-select actions and disables update for tags/remotes', () => {
    renderMenu(tag('v1.0'), [tag('v1.0'), remote('origin/main')]);
    expect(menuLabels()).toEqual([
      'Compare Branches',
      'Show Files Diff',
      'Update Selected',
      'Delete Branches⌘⌫'
    ]);
    const update = screen.getByRole('button', { name: 'Update Selected' }) as HTMLButtonElement;
    expect(update.disabled).toBe(true);
    const del = screen.getByRole('button', { name: /Delete Branches/ }) as HTMLButtonElement;
    expect(del.disabled).toBe(true);
  });

  it('passes exactly two selected refs to compare and files diff', () => {
    const a = local('a');
    const b = local('b');
    renderMenu(a, [a, b]);
    fireEvent.click(screen.getByRole('button', { name: 'Compare Branches' }));
    expect(actions.onCompareBranches).toHaveBeenCalledWith(a, b);
    expect(actions.onShowFilesDiff).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole('button', { name: 'Show Files Diff' }));
    expect(actions.onShowFilesDiff).toHaveBeenCalledWith(a, b);
  });

  it('disables both compare actions when more than two refs are selected', () => {
    const a = local('a');
    const b = local('b');
    const c = local('c');
    renderMenu(a, [a, b, c]);
    expect((screen.getByRole('button', { name: 'Compare Branches' }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole('button', { name: 'Show Files Diff' }) as HTMLButtonElement).disabled).toBe(true);
  });
});
