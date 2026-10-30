// 应用设置对话框
//
// 主题属于应用设置，不应以「设计评审浮动条」的形式常驻界面之上。
// 本对话框是主题选择、终端内核、字号等偏好的唯一入口。

import React, { useState } from 'react';
import { IconWindowClose } from './FileIcons';
import {
  DEFAULT_SETTINGS,
  loadSettings,
  persistSettings,
  type AppSettings
} from '../services/appSettings';
// 本面板自身的文案也走字典：语言下拉必须在当前语言下可读，
// 否则用户看不懂自己刚把语言切成了什么。
import { getLanguage, LANGUAGE_LABELS, setLanguage, subscribeLanguage, normalizeLanguage, t, type Language } from '../services/i18n';
import { APP_VERSION } from '../services/appVersion';
import { invokeCommand } from '../services/tauriBridge';
import { importFromVSCode } from '../services/settingsLayers';
import { isTelemetryEnabled, setTelemetryEnabled, trackEvent } from '../services/telemetry';
import { errText } from '../services/errorText';

// 存储层在 appSettings.ts（App 需要同步读取，不能等弹窗加载完）；
// 这里再导出一次，让既有 `from './components/SettingsDialog'` 的调用方
// 继续可用。弹窗本体已被 React.lazy 拆出，这些具名导出只在打开
// 设置面板时才进入产物。
export { DEFAULT_SETTINGS, loadSettings, persistSettings, type AppSettings };

export interface ThemeOption {
  id: number;
  name: string;
  desc: string;
  /** 用于列表预览：背景 / 面板 / 强调色 */
  swatch: [string, string, string];
}

/**
 * 编辑器配色列表。
 *
 * 必须是**函数**而不是模块级 const：name/desc 来自字典，而字典内容随语言切换。
 * 模块级 const 在 import 时求值一次，之后切语言不会重算 —— 用户会看到
 * 「界面已经是中文，主题名还是英文」。渲染时调用即可跟随当前语言。
 */
export const themeOptions = (): ThemeOption[] => [
  {
    id: 0,
    name: t().settingsPanel.themeFollowUi,
    desc: t().settingsPanel.themeFollowUiDesc,
    swatch: ['#1e1e20', '#0a84ff', '#e8e8ea']
  },
  {
    id: 1,
    name: t().settingsPanel.themeIdeDark,
    desc: t().settingsPanel.themeIdeDarkDesc,
    swatch: ['#1e1f22', '#3574f0', '#bcbec4']
  },
  {
    id: 2,
    name: t().settingsPanel.themeIdeLight,
    desc: t().settingsPanel.themeIdeLightDesc,
    swatch: ['#f8f8f9', '#0071e3', '#1d1d1f']
  }
];

/** 界面风格变体：每套 = 配色令牌 + 玻璃材质 + 结构签名（见 theme.css 与 variants/） */
export interface UiVariantOption {
  id: number;
  name: string;
  tag: string;
  desc: string;
  swatch: [string, string, string];
}

/** 界面风格变体。同 themeOptions：渲染时求值以跟随语言。 */
export const uiVariants = (): UiVariantOption[] => [
  { id: 1, name: t().settingsPanel.variantNativeDark, tag: t().settingsPanel.variantNativeDarkTag, desc: t().settingsPanel.variantNativeDarkDesc, swatch: ['#1e1e20', '#2c2c2e', '#0a84ff'] },
  { id: 2, name: t().settingsPanel.variantNativeLight, tag: t().settingsPanel.variantNativeLightTag, desc: t().settingsPanel.variantNativeLightDesc, swatch: ['#ffffff', '#f7f7f9', '#0071e3'] }
];

interface SettingsDialogProps {
  open: boolean;
  settings: AppSettings;
  onChange: (next: AppSettings, changedKeys?: Array<keyof AppSettings>) => void;
  onClose: () => void;
  /** 引擎版本等信息，用于「关于」区块 */
  about?: { engineVersion?: string; ghosttyVersion?: string; themeCount?: number };
  /** 当前项目根（VSCode 导入读它的 .vscode/settings.json）；未打开项目时不传 */
  workspacePath?: string;
  /** `.lazecode/settings.json` 的项目层覆盖项 */
  projectOverrides?: Partial<AppSettings>;
  /** 清除项目层覆盖，回落到用户设置 */
  onClearProjectOverrides?: () => void;
  onCheckForUpdates?: () => void;
}

type Section = 'appearance' | 'terminal' | 'editor' | 'files' | 'updates' | 'about';

/** 分区标签。同 themeOptions：渲染时求值。 */
const sections = (): Array<{ id: Section; label: string }> => [
  { id: 'appearance', label: t().settingsPanel.appearanceTab },
  { id: 'terminal', label: t().settingsPanel.terminalTab },
  { id: 'editor', label: t().settingsPanel.editorTab },
  { id: 'files', label: t().settingsPanel.filesTab },
  { id: 'updates', label: t().settingsPanel.updatesTab },
  { id: 'about', label: t().settingsPanel.aboutTab }
];

const label: React.CSSProperties = { fontSize: '12px', color: 'var(--text-primary)' };
const hint: React.CSSProperties = { fontSize: '10px', color: 'var(--text-muted)', marginTop: '3px' };
const rowStyle: React.CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'space-between',
  gap: '12px',
  padding: '9px 0',
  borderBottom: '1px solid var(--border-subtle)'
};

export const SettingsDialog: React.FC<SettingsDialogProps> = ({
  open,
  settings,
  onChange,
  onClose,
  about,
  workspacePath = '',
  projectOverrides,
  onClearProjectOverrides,
  onCheckForUpdates
}) => {
  const [section, setSection] = useState<Section>('appearance');
  const [diagnosticsEnabled, setDiagnosticsEnabled] = useState<boolean>(() => isTelemetryEnabled());
  // 语言要单独处理而不能只 patch：setLanguage 会同时落盘 settings、
  // 驱动字典切换、并让 Rust 重建原生菜单。只 patch 的话界面文案不更新，
  // 原生菜单也停在旧语言（直到下次重启）。
  //
  // 订阅语言变更来触发重渲染：字典是模块级变量（非 React state），
  // 不订阅的话本面板的 `t()` 结果在切换后不会重新求值。
  const [currentLang, setCurrentLang] = useState<Language>(() => getLanguage());
  React.useEffect(() => {
    return subscribeLanguage(l => {
      setCurrentLang(l);
    });
  }, []);
  // currentLang 只用于让本组件在语言变化时重跑渲染（Dictionary 是模块级的）。
  void currentLang;

  const changeLanguage = (next: Language) => {
    setLanguage(next);
    patch({ language: next });
  };
  const [newPattern, setNewPattern] = useState('');
  const currentPatterns = settings.ignoredPatterns || DEFAULT_SETTINGS.ignoredPatterns;

  const handleAddPattern = () => {
    const trimmed = newPattern.trim();
    if (!trimmed) return;
    if (currentPatterns.includes(trimmed)) return;
    patch({ ignoredPatterns: [...currentPatterns, trimmed] });
    setNewPattern('');
  };

  const handleRemovePattern = (pat: string) => {
    patch({ ignoredPatterns: currentPatterns.filter(p => p !== pat) });
  };

  const handleRestorePatterns = () => {
    patch({ ignoredPatterns: [...DEFAULT_SETTINGS.ignoredPatterns] });
  };

  const [importMsg, setImportMsg] = useState<string | null>(null);
  const [importing, setImporting] = useState(false);
  // 从 VSCode 导入：读当前项目的 .vscode/settings.json，只取本应用有的映射项
  //（见 settingsLayers.importFromVSCode）。写入走 onChange 即 user 层，
  // 与手动调节的语义一致；项目层（.lazecode/）不受影响。
  const handleImportVSCode = () => {
    if (importing) return;
    if (!workspacePath) {
      setImportMsg(t().settingsPanel.vscodeImportNoWorkspace);
      return;
    }
    setImporting(true);
    setImportMsg(null);
    const vscodePath = `${workspacePath.replace(/[\\/]+$/, '')}/.vscode/settings.json`;
    void invokeCommand<string>('read_file_content', { path: vscodePath }).then(
      text => {
        let parsed: unknown;
        try {
          parsed = JSON.parse(text);
        } catch (e) {
          setImportMsg(t().settingsPanel.vscodeImportFailed(e instanceof Error ? e.message : String(e)));
          return;
        }
        const { partial, mapped, unmapped } = importFromVSCode(parsed, normalizeLanguage);
        if (mapped.length === 0) {
          setImportMsg(t().settingsPanel.vscodeImportNone);
          return;
        }
          onChange({ ...settings, ...partial }, Object.keys(partial) as Array<keyof AppSettings>);
          setImportMsg(t().settingsPanel.vscodeImportOk(mapped.length, unmapped.join(', ')));
          // 遥测：只记导入个数，不记 key 名与值（隐私，见 telemetry.ts）
          trackEvent('Settings Imported', { count: mapped.length });
      },
      err => {
        setImportMsg(t().settingsPanel.vscodeImportFailed(errText(err)));
      },
    ).finally(() => {
      setImporting(false);
    });
  };

  if (!open) return null;

  const patch = (part: Partial<AppSettings>) =>
    onChange({ ...settings, ...part }, Object.keys(part) as Array<keyof AppSettings>);

  return (
    <div
      className="py-dialog-backdrop settings-dialog-backdrop"
      style={{
        position: 'fixed',
        inset: 0,
        background: 'var(--scrim, rgba(0, 0, 0, 0.55))',
        zIndex: 2500,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center'
      }}
    >
      <div
        className="py-dialog-window settings-dialog-window"
        style={{
          width: '720px',
          height: '520px',
          background: 'var(--bg-dialog, #2b2d30)',
          border: '1px solid var(--border-color)',
          borderRadius: 'var(--radius-lg, 8px)',
          boxShadow: '0 18px 48px var(--scrim-strong, rgba(0, 0, 0, 0.55))',
          display: 'flex',
          overflow: 'hidden'
        }}
      >
        {/* 左侧分区导航 */}
        <nav
          className="settings-dialog-sidebar"
          style={{
            width: '168px',
            flexShrink: 0,
            background: 'var(--bg-dialog-sidebar, #212225)',
            borderRight: '1px solid var(--border-color)',
            padding: '10px 0'
          }}
        >
          <div
            style={{
              padding: '0 14px 10px',
              fontSize: '11px',
              fontWeight: 700,
              color: 'var(--text-secondary)',
              letterSpacing: '0.4px'
            }}
          >
            {t().settingsPanel.settingsTitle}
          </div>
          {sections().map(s => {
            const active = s.id === section;
            return (
              <button
                key={s.id}
                onClick={() => setSection(s.id)}
                style={{
                  display: 'block',
                  width: '100%',
                  textAlign: 'left',
                  padding: '7px 14px',
                  border: 'none',
                  background: active ? 'var(--bg-selected)' : 'transparent',
                  color: active ? 'var(--text-primary)' : 'var(--text-secondary)',
                  borderLeft: active ? '2px solid var(--accent)' : '2px solid transparent',
                  fontSize: '12px',
                  cursor: 'pointer'
                }}
              >
                {s.label}
              </button>
            );
          })}
        </nav>

        {/* 右侧内容 */}
        <div style={{ flex: 1, display: 'flex', flexDirection: 'column', minWidth: 0, background: 'var(--bg-dialog, #2b2d30)' }}>
          <header
            style={{
              height: '40px',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              padding: '0 14px',
              borderBottom: '1px solid var(--border-color)',
              flexShrink: 0,
              background: 'var(--bg-dialog, #2b2d30)'
            }}
          >
            <span style={{ fontSize: '13px', fontWeight: 600, color: 'var(--text-primary)' }}>
              {sections().find(s => s.id === section)?.label}
            </span>
            <button
              onClick={onClose}
              style={{
                background: 'transparent',
                border: 'none',
                color: 'var(--text-muted)',
                fontSize: '15px',
                cursor: 'pointer'
              }}
              title={t().settingsPanel.closeEsc}
            >
              <IconWindowClose size={14} />
            </button>
          </header>

          <div style={{ flex: 1, overflowY: 'auto', padding: '14px 18px', background: 'var(--bg-dialog, #2b2d30)' }}>
            {projectOverrides && Object.keys(projectOverrides).length > 0 && (
              <div className="settings-project-overrides">
                <div>
                  <div style={{ fontSize: 12, fontWeight: 600, color: 'var(--text-primary)' }}>
                    {t().settingsPanel.projectOverrides}
                  </div>
                  <div style={{ fontSize: 11, color: 'var(--text-muted)', marginTop: 2 }}>
                    {t().settingsPanel.projectOverridesHint(Object.keys(projectOverrides).length)}
                  </div>
                </div>
                {onClearProjectOverrides && (
                  <button type="button" className="py-btn sm" onClick={onClearProjectOverrides}>
                    {t().settingsPanel.clearProjectOverrides}
                  </button>
                )}
              </div>
            )}
            {section === 'appearance' && (
              <>
                <div style={{ ...label, fontWeight: 600, marginBottom: '8px' }}>{t().settingsPanel.uiVariantLabel}</div>
                <div style={{ display: 'grid', gridTemplateColumns: '1fr', gap: '6px', marginBottom: '18px' }}>
                  {uiVariants().map(v => {
                    const active = v.id === settings.uiVariant;
                    return (
                      <button
                        key={v.id}
                        onClick={() => patch({ uiVariant: v.id })}
                        style={{
                          display: 'flex',
                          alignItems: 'center',
                          gap: '10px',
                          padding: '8px 10px',
                          textAlign: 'left',
                          background: active ? 'var(--bg-selected)' : 'var(--bg-dialog-card, #313438)',
                          border: `1px solid ${active ? 'var(--accent)' : 'var(--border-subtle)'}`,
                          borderRadius: 'var(--radius-md, 6px)',
                          cursor: 'pointer'
                        }}
                      >
                        <span style={{ display: 'flex', gap: '3px', flexShrink: 0 }}>
                          {v.swatch.map(c => (
                            <span
                              key={c}
                              style={{
                                width: 12,
                                height: 12,
                                borderRadius: 3,
                                background: c,
                                border: '1px solid rgba(128,128,128,0.4)'
                              }}
                            />
                          ))}
                        </span>
                        <span style={{ flex: 1, minWidth: 0 }}>
                          <span style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
                            <span style={{ fontSize: '12px', fontWeight: active ? 600 : 500, color: 'var(--text-primary)' }}>
                              {v.name}
                            </span>
                            <span
                              style={{
                                fontSize: '9px',
                                padding: '1px 5px',
                                borderRadius: 4,
                                color: 'var(--text-secondary)',
                                background: 'var(--bg-hover)'
                              }}
                            >
                              {v.tag}
                            </span>
                          </span>
                          <span style={{ display: 'block', fontSize: '10px', color: 'var(--text-muted)', marginTop: '2px' }}>
                            {v.desc}
                          </span>
                        </span>
                        {active && <span style={{ color: 'var(--accent)', fontSize: 12 }}>✓</span>}
                      </button>
                    );
                  })}
                </div>

                <div style={{ ...label, fontWeight: 600, marginBottom: '8px' }}>{t().settingsPanel.editorThemeLabel}</div>
                <div style={{ display: 'grid', gridTemplateColumns: '1fr', gap: '8px' }}>
                  {themeOptions().map(opt => {
                    const active = opt.id === settings.themeId;
                    return (
                      <button
                        key={opt.id}
                        onClick={() => patch({ themeId: opt.id })}
                        style={{
                          display: 'flex',
                          alignItems: 'center',
                          gap: '10px',
                          padding: '9px 10px',
                          textAlign: 'left',
                          background: active ? 'var(--bg-selected)' : 'var(--bg-dialog-card, #313438)',
                          border: `1px solid ${active ? 'var(--accent)' : 'var(--border-subtle)'}`,
                          borderRadius: '6px',
                          cursor: 'pointer'
                        }}
                      >
                        {/* 三色预览 */}
                        <span style={{ display: 'flex', flexShrink: 0 }}>
                          {opt.swatch.map((c, i) => (
                            <span
                              key={i}
                              style={{
                                width: '16px',
                                height: '32px',
                                background: c,
                                borderRadius:
                                  i === 0 ? '4px 0 0 4px' : i === 2 ? '0 4px 4px 0' : undefined,
                                border: i === 1 ? '1px solid var(--overlay-hover)' : undefined
                              }}
                            />
                          ))}
                        </span>
                        <span style={{ minWidth: 0 }}>
                          <span
                            style={{
                              display: 'block',
                              fontSize: '12px',
                              color: 'var(--text-primary)',
                              fontWeight: active ? 600 : 400
                            }}
                          >
                            {opt.name}
                          </span>
                          <span
                            style={{
                              display: 'block',
                              fontSize: '10px',
                              color: 'var(--text-muted)',
                              lineHeight: '14px'
                            }}
                          >
                            {opt.desc}
                          </span>
                        </span>
                      </button>
                    );
                  })}
                </div>

                <div style={{ ...rowStyle, marginTop: '18px' }}>
                  <div>
                    <div style={label}>{t().settings.language}</div>
                    <div style={hint}>{t().settings.languageHint}</div>
                  </div>
                  <select
                    value={settings.language}
                    onChange={e => changeLanguage(e.target.value as Language)}
                    aria-label={t().settings.language}
                    style={{
                      background: 'var(--bg-dialog-input, #1e1f22)',
                      border: '1px solid var(--border-subtle)',
                      borderRadius: '4px',
                      color: 'var(--text-primary)',
                      fontSize: '12px',
                      padding: '4px 8px'
                    }}
                  >
                    {/* 语言名用各自语言自称（English / 简体中文），
                        不翻译成当前语言 —— 否则切到法语时用户看不懂 "Chinois"。 */}
                    <option value="en">{LANGUAGE_LABELS.en}</option>
                    <option value="zh-CN">{LANGUAGE_LABELS['zh-CN']}</option>
                  </select>
                </div>

                <div style={{ ...rowStyle, marginTop: '18px' }}>
                  <div>
                    <div style={label}>{t().settingsPanel.windowDecoration}</div>
                    <div style={hint}>{t().settingsPanel.windowDecorationHint}</div>
                  </div>
                  <select
                    value={settings.captionStyle}
                    onChange={e => patch({ captionStyle: e.target.value as 'macos' | 'windows' })}
                    style={{
                      background: 'var(--bg-dialog-input, #1e1f22)',
                      border: '1px solid var(--border-subtle)',
                      borderRadius: '4px',
                      color: 'var(--text-primary)',
                      fontSize: '12px',
                      padding: '4px 8px'
                    }}
                  >
                    <option value="macos">{t().settingsPanel.captionMacos}</option>
                    <option value="windows">{t().settingsPanel.captionWindows}</option>
                  </select>
                </div>
                <div style={{ ...rowStyle, marginTop: '12px' }}>
                  <div>
                    <div style={label}>{t().settingsPanel.uiFontSizeLabel}</div>
                    <div style={hint}>{t().settingsPanel.uiFontSizeHint}</div>
                  </div>
                  <select
                    value={settings.uiFontSize}
                    onChange={e => patch({ uiFontSize: Number(e.target.value) })}
                    style={{
                      background: 'var(--bg-dialog-input, #1e1f22)',
                      border: '1px solid var(--border-subtle)',
                      borderRadius: '4px',
                      color: 'var(--text-primary)',
                      fontSize: '12px',
                      padding: '4px 8px'
                    }}
                  >
                    {[13, 14, 15, 16].map(px => (
                      <option key={px} value={px}>
                        {px} px{px === 14 ? t().settingsPanel.uiFontSizeDefault : ''}
                      </option>
                    ))}
                  </select>
                </div>
              </>
            )}

            {section === 'terminal' && (
              <>
                <div style={rowStyle}>
                  <div>
                    <div style={label}>{t().settingsPanel.terminalEngine}</div>
                    <div style={hint}>
                      {t().settingsPanel.terminalEngineHint}
                    </div>
                  </div>
                  <span style={{ fontSize: '12px', color: 'var(--text-secondary)' }}>libghostty-vt</span>
                </div>

                <div style={rowStyle}>
                  <div>
                    <div style={label}>{t().settingsPanel.terminalFontSizeLabel}</div>
                    <div style={hint}>{t().settingsPanel.terminalFontSizeHint}</div>
                  </div>
                  <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                    <input
                      type="range"
                      min={10}
                      max={20}
                      value={settings.terminalFontSize}
                      onChange={e => patch({ terminalFontSize: Number(e.target.value) })}
                    />
                    <span style={{ fontSize: '12px', color: 'var(--text-secondary)', width: '30px' }}>
                      {settings.terminalFontSize}px
                    </span>
                  </div>
                </div>
              </>
            )}

            {section === 'editor' && (
              <>
                <div style={rowStyle}>
                  <div>
                    <div style={label}>{t().settingsPanel.editorFontSizeLabel}</div>
                    <div style={hint}>{t().settingsPanel.editorFontSizeHint}</div>
                  </div>
                  <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                    <input
                      type="range"
                      min={10}
                      max={22}
                      value={settings.editorFontSize}
                      onChange={e => patch({ editorFontSize: Number(e.target.value) })}
                    />
                    <span style={{ fontSize: '12px', color: 'var(--text-secondary)', width: '30px' }}>
                      {settings.editorFontSize}px
                    </span>
                  </div>
                </div>
                <div style={{ ...rowStyle, borderBottom: 'none' }}>
                  <div>
                    <div style={label}>{t().settingsPanel.vscodeImport}</div>
                    <div style={hint}>{t().settingsPanel.vscodeImportHint}</div>
                    {importMsg && (
                      <div style={{ ...hint, color: 'var(--text-secondary)' }}>{importMsg}</div>
                    )}
                  </div>
                  <button
                    type="button"
                    disabled={importing}
                    onClick={handleImportVSCode}
                    style={{
                      background: 'var(--bg-dialog-input, #1e1f22)',
                      border: '1px solid var(--border-subtle)',
                      borderRadius: '4px',
                      color: 'var(--text-primary)',
                      fontSize: '12px',
                      padding: '4px 12px',
                      cursor: importing ? 'wait' : 'pointer',
                      opacity: importing ? 0.6 : 1,
                      whiteSpace: 'nowrap'
                    }}
                  >
                    {importing ? '…' : t().settingsPanel.vscodeImport}
                  </button>
                </div>
              </>
            )}
            {section === 'files' && (
              <>
                <div style={{ marginBottom: '14px' }}>
                  <div style={{ ...label, fontWeight: 600, fontSize: '13px' }}>
                    {t().settingsPanel.ignoredFilesTitle}
                  </div>
                  <div style={{ ...hint, marginTop: '4px', lineHeight: '1.5' }}>
                    {t().settingsPanel.ignoredFilesDesc}
                  </div>
                </div>

                <div style={{ display: 'flex', gap: '8px', marginBottom: '14px' }}>
                  <input
                    type="text"
                    value={newPattern}
                    onChange={e => setNewPattern(e.target.value)}
                    onKeyDown={e => {
                      if (e.key === 'Enter') {
                        e.preventDefault();
                        handleAddPattern();
                      }
                    }}
                    placeholder={t().settingsPanel.ignoredPatternPlaceholder}
                    style={{
                      flex: 1,
                      background: 'var(--bg-dialog-input, #1e1f22)',
                      border: '1px solid var(--border-subtle)',
                      borderRadius: '4px',
                      fontSize: '12px',
                      padding: '5px 10px',
                      outline: 'none'
                    }}
                  />
                  <button
                    type="button"
                    disabled={!newPattern.trim()}
                    onClick={handleAddPattern}
                    style={{
                      background: 'var(--accent)',
                      border: 'none',
                      borderRadius: '4px',
                      color: '#ffffff',
                      fontSize: '12px',
                      padding: '5px 14px',
                      cursor: newPattern.trim() ? 'pointer' : 'default',
                      opacity: newPattern.trim() ? 1 : 0.5,
                      whiteSpace: 'nowrap'
                    }}
                  >
                    {t().settingsPanel.addPatternBtn}
                  </button>
                  <button
                    type="button"
                    onClick={handleRestorePatterns}
                    style={{
                      background: 'var(--bg-dialog-input, #1e1f22)',
                      border: '1px solid var(--border-subtle)',
                      borderRadius: '4px',
                      color: 'var(--text-primary)',
                      fontSize: '12px',
                      padding: '5px 12px',
                      cursor: 'pointer',
                      whiteSpace: 'nowrap'
                    }}
                    title={t().settingsPanel.restoreDefaultPatterns}
                  >
                    {t().settingsPanel.restoreDefaultPatterns}
                  </button>
                </div>

                <div
                  style={{
                    display: 'flex',
                    flexWrap: 'wrap',
                    gap: '6px',
                    padding: '12px',
                    background: 'var(--bg-dialog-input, #1e1f22)',
                    border: '1px solid var(--border-subtle)',
                    borderRadius: '6px',
                    minHeight: '120px',
                    alignContent: 'flex-start'
                  }}
                >
                  {currentPatterns.map(pat => (
                    <span
                      key={pat}
                      style={{
                        display: 'inline-flex',
                        alignItems: 'center',
                        gap: '6px',
                        background: 'var(--bg-dialog-card, #313438)',
                        border: '1px solid var(--border-color)',
                        borderRadius: '3px',
                        padding: '3px 8px',
                        fontSize: '12px',
                        fontFamily: 'var(--font-code)',
                        color: 'var(--text-primary)'
                      }}
                    >
                      <span>{pat}</span>
                      <button
                        type="button"
                        onClick={() => handleRemovePattern(pat)}
                        style={{
                          background: 'none',
                          border: 'none',
                          color: 'var(--text-muted)',
                          cursor: 'pointer',
                          padding: 0,
                          fontSize: '13px',
                          lineHeight: 1,
                          display: 'inline-flex',
                          alignItems: 'center'
                        }}
                        title={t().common.close}
                      >
                        ×
                      </button>
                    </span>
                  ))}
                </div>
              </>
            )}
            {section === 'updates' && (
              <>
                <div style={rowStyle}>
                  <div>
                    <div style={label}>{t().settingsPanel.autoCheckUpdates}</div>
                    <div style={hint}>{t().settingsPanel.autoCheckUpdatesDesc}</div>
                  </div>
                  <input
                    type="checkbox"
                    checked={settings.autoCheckUpdates}
                    onChange={e => patch({ autoCheckUpdates: e.target.checked })}
                    style={{ cursor: 'pointer', accentColor: 'var(--accent-color)' }}
                  />
                </div>

                <div style={rowStyle}>
                  <div>
                    <div style={label}>
                      {t().settingsPanel.lastChecked(
                        settings.lastUpdateCheckTime
                          ? new Date(settings.lastUpdateCheckTime).toLocaleString()
                          : t().settingsPanel.neverChecked
                      )}
                    </div>
                    {settings.ignoredUpdateVersion ? (
                      <div style={hint}>
                        {t().updater.ignoreThisVersion}：{settings.ignoredUpdateVersion}{' '}
                        <button
                          type="button"
                          onClick={() => patch({ ignoredUpdateVersion: '' })}
                          style={{
                            background: 'none',
                            border: 'none',
                            color: 'var(--accent-color)',
                            cursor: 'pointer',
                            fontSize: '11px',
                            padding: 0,
                            textDecoration: 'underline'
                          }}
                        >
                          {t().common.close}
                        </button>
                      </div>
                    ) : null}
                  </div>
                  <button
                    type="button"
                    onClick={() => {
                      onClose();
                      onCheckForUpdates?.();
                    }}
                    style={{
                      background: 'var(--bg-dialog-input, #1e1f22)',
                      border: '1px solid var(--border-subtle)',
                      borderRadius: '4px',
                      color: 'var(--text-primary)',
                      fontSize: '12px',
                      padding: '5px 14px',
                      cursor: 'pointer',
                      whiteSpace: 'nowrap'
                    }}
                  >
                    {t().settingsPanel.checkNow}
                  </button>
                </div>
              </>
            )}

            {section === 'about' && (
              <div style={{ fontSize: '12px', color: 'var(--text-secondary)', lineHeight: '22px' }}>
                <div style={{ fontSize: '13px', color: 'var(--text-primary)', fontWeight: 600 }}>
                  LazeCode
                </div>
                <div style={{ marginTop: '2px', color: 'var(--text-muted)' }}>{t().settingsPanel.aboutStack(APP_VERSION)}</div>
                <div style={{ marginTop: '14px' }}>
                  <div>
                    {t().settingsPanel.aboutTerminalEngine}：libghostty-vt <span style={{ color: 'var(--text-muted)' }}>{about?.engineVersion ?? '—'}</span>
                  </div>
                  <div>
                    {t().settingsPanel.aboutLocalGhostty}：
                    <span style={{ color: about?.ghosttyVersion ? 'var(--text-primary)' : 'var(--text-muted)' }}>
                      {about?.ghosttyVersion ?? t().settingsPanel.engineNotDetected}
                    </span>
                    {about?.themeCount ? (
                      <span style={{ color: 'var(--text-muted)' }}>{t().settingsPanel.themeCount(about.themeCount)}</span>
                    ) : null}
                  </div>
                  <div style={{ marginTop: '10px', color: 'var(--text-muted)' }}>
                    {t().settingsPanel.aboutRenderPipeline}
                  </div>
                </div>
                <div style={{ ...rowStyle, marginTop: '14px' }}>
                  <div>
                    <div style={label}>{t().settingsPanel.localDiagnostics}</div>
                    <div style={hint}>{t().settingsPanel.localDiagnosticsDesc}</div>
                  </div>
                  <input
                    type="checkbox"
                    checked={diagnosticsEnabled}
                    onChange={e => {
                      setDiagnosticsEnabled(e.target.checked);
                      setTelemetryEnabled(e.target.checked);
                    }}
                    style={{ cursor: 'pointer', accentColor: 'var(--accent-color)' }}
                  />
                </div>
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
};
