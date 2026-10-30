import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render } from '@testing-library/react';
import { ProjectDrawer } from './ProjectDrawer';
import type { FileNode } from '../types';

function fileNode(path: string, name: string): FileNode {
  return { name, path, isDirectory: false };
}

function renderDrawer(options: {
  rootPath: string;
  files: FileNode[];
  singleFile: boolean;
  expandedPaths?: Set<string>;
  onSelectFile?: (path: string) => void;
}) {
  return render(
    <ProjectDrawer
      rootName={options.rootPath.split('/').filter(Boolean).pop() ?? options.rootPath}
      rootPath={options.rootPath}
      files={options.files}
      singleFile={options.singleFile}
      activeFile={options.files[0]?.path ?? ''}
      expandedPaths={options.expandedPaths ?? new Set()}
      onSetExpandedPaths={vi.fn()}
      onSelectFile={options.onSelectFile ?? vi.fn()}
      onOpenDirectory={vi.fn()}
    />
  );
}

describe('ProjectDrawer single-file workspace', () => {
  it('renders the file row instead of a collapsible project folder', () => {
    const path = '/Users/tester/Documents/brandable_domains.txt';
    const { container } = renderDrawer({
      rootPath: path,
      files: [fileNode(path, 'brandable_domains.txt')],
      singleFile: true
    });

    expect(container.querySelector('.tree-root-item')).toBeNull();
    const row = container.querySelector('.tree-node-row');
    expect(row).not.toBeNull();
    expect(row?.classList.contains('is-file')).toBe(true);
    expect(row?.classList.contains('is-dir')).toBe(false);
    expect(container.querySelector('.tree-chevron')).toBeNull();
  });

  it('still renders a collapsible root for directory workspaces', () => {
    const { container } = renderDrawer({
      rootPath: '/repo',
      files: [{ name: 'src', path: '/repo/src', isDirectory: true, children: [] }],
      singleFile: false,
      expandedPaths: new Set(['/repo'])
    });

    expect(container.querySelector('.tree-root-item')).not.toBeNull();
    expect(container.querySelector('.tree-root-name')?.textContent).toBe('repo');
  });

  it('selects a file on single click and opens it on double click', () => {
    const onSelectFile = vi.fn();
    const path = '/repo/src/a.ts';
    const { container } = renderDrawer({
      rootPath: '/repo',
      files: [
        {
          name: 'src',
          path: '/repo/src',
          isDirectory: true,
          children: [fileNode(path, 'a.ts')]
        }
      ],
      singleFile: false,
      expandedPaths: new Set(['/repo', '/repo/src']),
      onSelectFile
    });

    const row = container.querySelector(`[data-tree-path="${path}"]`) as HTMLElement;
    expect(row).not.toBeNull();

    fireEvent.click(row);
    expect(onSelectFile).not.toHaveBeenCalled();
    expect(row).toHaveClass('selected');

    fireEvent.doubleClick(row);
    expect(onSelectFile).toHaveBeenCalledWith(path);
  });
});
