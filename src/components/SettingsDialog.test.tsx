import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { SettingsDialog } from './SettingsDialog';
import { DEFAULT_SETTINGS } from '../services/appSettings';

describe('SettingsDialog appearance', () => {
  it('renders exactly one editor color scheme section', () => {
    render(
      <SettingsDialog
        open
        settings={DEFAULT_SETTINGS}
        onChange={vi.fn()}
        onClose={vi.fn()}
      />
    );
    expect(screen.getAllByText('Editor color scheme')).toHaveLength(1);
    expect(screen.getByText('macOS Native Dark')).toBeInTheDocument();
  });
});
