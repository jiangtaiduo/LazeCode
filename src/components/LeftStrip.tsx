import React from 'react';
import {
  ProjectStripeIcon,
  CommitStripeIcon,
  CompareStripeIcon,
  GitLogStripeIcon,
  TerminalStripeIcon,
  SettingsStripeIcon
} from './FileIcons';
import { t } from '../services/i18n';

export type ActiveDrawer = 'commit' | 'project' | 'compare' | null;
export type BottomTool = 'git' | 'terminal' | 'vt-terminal' | null;

interface LeftStripProps {
  activeDrawer: ActiveDrawer;
  activeBottomTool: BottomTool;
  onToggleDrawer: (drawer: ActiveDrawer) => void;
  onToggleBottomTool: (tool: BottomTool) => void;
  onOpenSettings: () => void;
  settingsOpen: boolean;
  /** 变更清单文件数：> 0 时在 Commit 图标右上角亮指示点 */
  changeCount?: number;
  /** 是否由 Git 管理：非 git 管理的目录或文件不显示 Commit 图标 */
  isGitRepo?: boolean;
  /** 是否有活跃的提交版本比较会话 */
  hasCompareSession?: boolean;
  compareTooltip?: string;
  /** 当前项目控制台正在运行的命令名（去重）：非空时在 Terminal 图标上亮运行角标 */
  terminalRunning?: string[];
}
export const LeftStrip: React.FC<LeftStripProps> = ({
  activeDrawer,
  activeBottomTool,
  onToggleDrawer,
  onToggleBottomTool,
  onOpenSettings,
  settingsOpen,
  changeCount = 0,
  isGitRepo = false,
  hasCompareSession = false,
  compareTooltip = 'Compare Versions',
  terminalRunning = []
}) => {
  return (
    <nav className="tool-sidebar-strip" id="leftStrip" data-tauri-drag-region="false">
      {/* Top Tool Windows */}
      <div className="strip-top">
        {/* 1. Project Tool Window */}
        <button
          className={`strip-btn ${activeDrawer === 'project' ? 'active' : ''}`}
          onClick={() => onToggleDrawer(activeDrawer === 'project' ? null : 'project')}
          title={t().leftStrip.projectTooltip}
        >
          <ProjectStripeIcon size={17} color='currentColor' />
        </button>

        {/* 2. Commit / Changes Tool Window - 仅在 git 管理的目录或文件下显示 */}
        {isGitRepo && (
          <button
            className={`strip-btn ${activeDrawer === 'commit' ? 'active' : ''}`}
            onClick={() => onToggleDrawer(activeDrawer === 'commit' ? null : 'commit')}
            title={changeCount > 0 ? t().leftStrip.commitTooltipWithChanges(changeCount) : t().leftStrip.commitTooltip}
          >
            <CommitStripeIcon size={17} color='currentColor' />
            {changeCount > 0 && <span className="strip-dot" aria-hidden="true" />}
          </button>
        )}

        {/* 3. Compare Versions Tool Window - 对比两个提交时显示 (对齐 PyCharm 截图 Image #2) */}
        {hasCompareSession && (
          <button
            className={`strip-btn ${activeDrawer === 'compare' ? 'active' : ''}`}
            onClick={() => onToggleDrawer(activeDrawer === 'compare' ? null : 'compare')}
            title={compareTooltip}
          >
            <CompareStripeIcon size={17} color='currentColor' />
          </button>
        )}
      </div>

      {/* Bottom Tool Windows */}
      <div className="strip-bottom">
        {/* 5. Git Log Bottom Tool */}
        <button
          className={`strip-btn ${activeBottomTool === 'git' ? 'active' : ''}`}
          onClick={() => onToggleBottomTool(activeBottomTool === 'git' ? null : 'git')}
          title={t().leftStrip.gitLogTooltip}
        >
          <GitLogStripeIcon size={17} color='currentColor' />
        </button>

        {/* 6. Terminal Console Bottom Tool */}
        <button
          className={`strip-btn ${activeBottomTool === 'terminal' ? 'active' : ''}`}
          onClick={() => onToggleBottomTool(activeBottomTool === 'terminal' ? null : 'terminal')}
          title={terminalRunning.length > 0 ? t().leftStrip.terminalTooltipRunning(terminalRunning.slice(0, 3).join('、')) : t().leftStrip.terminalTooltip}
        >
          <TerminalStripeIcon size={17} color='currentColor' />
          {terminalRunning.length > 0 && <span className="strip-dot strip-dot-running" aria-hidden="true" />}
        </button>

        {/* 7. Settings */}
        <button
          className={`strip-btn ${settingsOpen ? 'active' : ''}`}
          onClick={onOpenSettings}
          title={t().leftStrip.settingsTooltip}
        >
          <SettingsStripeIcon size={17} color='currentColor' />
        </button>
      </div>
    </nav>
  );
};
