import React from 'react';
import {
  IconSearch,
  ProjectStripeIcon,
  IconWindowClose,
  FolderIcon,
  IconNewWindow,
  TerminalStripeIcon,
  SettingsStripeIcon
} from './FileIcons';
import { t } from '../services/i18n';
import { APP_VERSION } from '../services/appVersion';

interface WelcomeViewProps {
  onOpenProject: () => void;
  onCloneRepository?: () => void;
  onOpenRecent: (path: string) => void;
  onRemoveRecent: (path: string, e: React.MouseEvent) => void;
  recentProjects: string[];
  onSearchClick: () => void;
  onOpenTerminal: () => void;
  onOpenSettings: () => void;
  onNewWindow?: () => void;
}

export const WelcomeView: React.FC<WelcomeViewProps> = ({
  onOpenProject,
  onCloneRepository,
  onOpenRecent,
  onRemoveRecent,
  recentProjects,
  onSearchClick,
  onOpenTerminal,
  onOpenSettings,
  onNewWindow
}) => {
  return (
    <div className="welcome-view">
      <div className="welcome-container">
        {/* Header / Brand */}
        <div className="welcome-brand">
          <div className="welcome-logo-box">
            <span className="welcome-logo-icon"><ProjectStripeIcon size={24} color="#ffffff" /></span>
          </div>
          <h1 className="welcome-title">LazeCode</h1>
          <div className="welcome-version">v{APP_VERSION}</div>
          <p className="welcome-subtitle">
            {t().welcome.subtitle}
          </p>
        </div>

        <div className="welcome-content-grid">
          {/* Quick Start Actions */}
          <div className="welcome-card welcome-actions-card">
            <div className="welcome-card-header">
              <span className="welcome-card-title">{t().welcome.quickStart}</span>
            </div>
            <div className="welcome-actions-list">
              <button className="welcome-action-btn" onClick={onOpenProject}>
                <span className="action-icon"><FolderIcon size={16} color="var(--text-secondary)" /></span>
                <div className="action-text">
                  <div className="action-main">{t().welcome.openWorkspace}</div>
                  <div className="action-sub">{t().welcome.openWorkspaceSub}</div>
                </div>
              </button>

              {onCloneRepository && (
                <button className="welcome-action-btn" onClick={onCloneRepository}>
                  <span className="action-icon"><TerminalStripeIcon size={16} color="var(--text-secondary)" /></span>
                  <div className="action-text">
                    <div className="action-main">{t().clone.title}</div>
                    <div className="action-sub">{t().clone.urlPlaceholder}</div>
                  </div>
                </button>
              )}

              {onNewWindow && (
                <button className="welcome-action-btn" onClick={onNewWindow}>
                  <span className="action-icon"><IconNewWindow size={15} color="var(--text-secondary)" /></span>
                  <div className="action-text">
                    <div className="action-main">{t().welcome.newWindow}</div>
                    <div className="action-sub">{t().welcome.newWindowSub}</div>
                  </div>
                  <kbd className="welcome-kbd">⇧ ⌘ N</kbd>
                </button>
              )}

              <button className="welcome-action-btn" onClick={onSearchClick}>
                <span className="action-icon"><IconSearch size={15} color="var(--text-secondary)" /></span>
                <div className="action-text">
                  <div className="action-main">{t().welcome.globalSearch}</div>
                  <div className="action-sub">{t().welcome.globalSearchSub}</div>
                </div>
                <kbd className="welcome-kbd">⇧ ⇧</kbd>
              </button>

              <button className="welcome-action-btn" onClick={onOpenTerminal}>
                <span className="action-icon"><TerminalStripeIcon size={15} color="var(--text-secondary)" /></span>
                <div className="action-text">
                  <div className="action-main">{t().welcome.openTerminal}</div>
                  <div className="action-sub">{t().welcome.openTerminalSub}</div>
                </div>
                <kbd className="welcome-kbd">{'⌥ F12 / ⌘ `'}</kbd>
              </button>

              <button className="welcome-action-btn" onClick={onOpenSettings}>
                <span className="action-icon"><SettingsStripeIcon size={15} color="var(--text-secondary)" /></span>
                <div className="action-text">
                  <div className="action-main">{t().welcome.settings}</div>
                  <div className="action-sub">{t().welcome.settingsSub}</div>
                </div>
                <kbd className="welcome-kbd">⌘ ,</kbd>
              </button>
            </div>
          </div>

          {/* Recent Projects */}
          <div className="welcome-card welcome-recents-card">
            <div className="welcome-card-header">
              <span className="welcome-card-title">{t().welcome.recentProjects}</span>
              {recentProjects.length > 0 && (
                <span className="welcome-recents-count">{t().welcome.projectCount(recentProjects.length)}</span>
              )}
            </div>

            {recentProjects.length === 0 ? (
              <div className="welcome-recents-empty">
                <div className="empty-icon"><ProjectStripeIcon size={28} color="var(--text-muted)" /></div>
                <div className="empty-text">{t().welcome.noRecentProjects}</div>
                <button className="welcome-link-btn" onClick={onOpenProject}>
                  {t().welcome.openDirNow}
                </button>
              </div>
            ) : (
              <div className="welcome-recents-list">
                {recentProjects.map(path => {
                  const name = path.split(/[\\/]/).filter(Boolean).pop() || path;
                  return (
                    <div
                      key={path}
                      className="welcome-recent-item"
                      onClick={() => onOpenRecent(path)}
                      title={path}
                    >
                      <span className="recent-item-icon"><FolderIcon size={16} color="var(--text-secondary)" /></span>
                      <div className="recent-item-info">
                        <div className="recent-item-name">{name}</div>
                        <div className="recent-item-path">{path}</div>
                      </div>
                      <button
                        className="recent-item-remove"
                        onClick={e => onRemoveRecent(path, e)}
                        title={t().welcome.removeFromRecords}
                      >
                        <IconWindowClose size={11} />
                      </button>
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        </div>

        {/* Shortcuts Cheat Sheet */}
        <div className="welcome-footer-hints">
          <div className="hint-item"><kbd>⇧ ⇧</kbd> <span>{t().welcome.searchHint}</span></div>
          <div className="hint-item"><kbd>⌘ ,</kbd> <span>{t().welcome.settingsHint}</span></div>
          <div className="hint-item"><kbd>ESC</kbd> <span>{t().welcome.closeDialogHint}</span></div>
        </div>
      </div>
    </div>
  );
};
