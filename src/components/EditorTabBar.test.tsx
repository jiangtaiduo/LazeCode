import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render } from '@testing-library/react';
import { EditorTabBar } from './EditorTabBar';
import { t } from '../services/i18n';

const noop = vi.fn();

function renderBar() {
  return render(
    <EditorTabBar
      openFiles={[]}
      activeFile=""
      activeTab="compare-branches"
      diffOpen
      compareBranches={{ left: 'main', right: 'dev' }}
      onSelectTab={noop}
      onSelectDiffTab={noop}
      onSelectCompareBranches={noop}
      onCloseCompareBranches={noop}
      onCloseTab={noop}
      onCloseOtherTabs={noop}
      onCloseLeftTabs={noop}
      onCloseRightTabs={noop}
      onCloseAllTabs={noop}
      onCloseDiffTab={noop}
    />
  );
}

describe('EditorTabBar', () => {
  beforeEach(() => {
    // jsdom 未实现标签栏自动滚动用的 scrollIntoView。
    HTMLElement.prototype.scrollIntoView = vi.fn();
  });

  it('places the Diff tab to the right of Compare Branches', () => {
    renderBar();
    const tabs = document.querySelectorAll('.ide-tab');
    expect(tabs).toHaveLength(2);
    expect(tabs[0]).toHaveAttribute(
      'title',
      t().compareBranches.tabTitle('main', 'dev')
    );
    expect(tabs[1]).toHaveAttribute('title', t().editorTab.diffTabTitle);
  });
});
