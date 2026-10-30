import React from 'react';
import { t } from '../services/i18n';
import { useLanguageRefresh } from '../hooks/useLanguageRefresh';

interface MacTrafficLightsProps {
  onClose?: () => void;
  onMinimize?: () => void;
  onMaximize?: () => void;
  isMaximized?: boolean;
}

export const MacTrafficLights: React.FC<MacTrafficLightsProps> = ({
  onClose,
  onMinimize,
  onMaximize,
  isMaximized = false
}) => {
  useLanguageRefresh();
  return (
    <div className="mac-traffic-lights">
      <button
        type="button"
        className="mac-traffic-btn close"
        onClick={onClose}
        title={t().common.close}
        aria-label={t().common.close}
      >
        <span className="mac-traffic-glyph">✕</span>
      </button>
      <button
        type="button"
        className="mac-traffic-btn minimize disabled"
        onClick={onMinimize}
        disabled
        title={t().common.minimize}
        aria-label={t().common.minimize}
      >
        <span className="mac-traffic-glyph">−</span>
      </button>
      <button
        type="button"
        className="mac-traffic-btn maximize"
        onClick={onMaximize}
        title={isMaximized ? t().common.restore : t().common.maximize}
        aria-label={isMaximized ? t().common.restore : t().common.maximize}
      >
        <span className="mac-traffic-glyph">{isMaximized ? '⤡' : '⤢'}</span>
      </button>
    </div>
  );
};
