import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { SearchEverywhereModal } from './SearchEverywhereModal';
import { t } from '../services/i18n';

afterEach(cleanup);

describe('SearchEverywhereModal actions', () => {
  it('lists built-in actions and runs the selected one', () => {
    const onRunAction = vi.fn();
    const onClose = vi.fn();
    render(
      <SearchEverywhereModal
        isOpen
        workspacePath=""
        openFiles={[]}
        onClose={onClose}
        onOpenFile={vi.fn()}
        onRunAction={onRunAction}
      />
    );

    fireEvent.click(screen.getByRole('button', { name: t().search.tabActions }));
    expect(screen.getByText(t().menu.commitEllipsis)).toBeInTheDocument();

    fireEvent.click(screen.getByText(t().menu.commitEllipsis));
    expect(onRunAction).toHaveBeenCalledWith('commit');
    expect(onClose).toHaveBeenCalled();
  });

  it('filters actions by the query', () => {
    render(
      <SearchEverywhereModal
        isOpen
        workspacePath=""
        openFiles={[]}
        onClose={vi.fn()}
        onOpenFile={vi.fn()}
        onRunAction={vi.fn()}
      />
    );

    fireEvent.click(screen.getByRole('button', { name: t().search.tabActions }));
    fireEvent.change(screen.getByPlaceholderText(t().search.tabActions), {
      target: { value: 'terminal' }
    });
    expect(screen.getByText(t().menu.terminal)).toBeInTheDocument();
    expect(screen.queryByText(t().menu.commitEllipsis)).not.toBeInTheDocument();
  });
});
