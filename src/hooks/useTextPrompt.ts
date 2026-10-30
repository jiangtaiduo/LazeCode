import { useState, useRef, useCallback } from 'react';
import type { TextPromptRequest } from '../components/TextPromptDialog';

export interface UseTextPromptReturn {
  textPromptReq: TextPromptRequest | null;
  appPrompt: (message: string, defaultValue?: string, opts?: { allowEmpty?: boolean }) => Promise<string | null>;
  resolveTextPrompt: (v: string | null) => void;
}

/**
 * 通用文本输入框（替代 window.prompt —— wry 未实现 JS 面板，prompt 直接回 null）：
 * 调用方 `await appPrompt(message, defaultValue)`，确认 resolve 输入值，
 * 取消/Esc resolve null。pending 中再调一次会作废前一次（单框语义）。
 */
export function useTextPrompt(): UseTextPromptReturn {
  const [textPromptReq, setTextPromptReq] = useState<TextPromptRequest | null>(null);
  const textPromptResolveRef = useRef<((v: string | null) => void) | null>(null);

  const appPrompt = useCallback(
    (message: string, defaultValue?: string, opts?: { allowEmpty?: boolean }): Promise<string | null> => {
      textPromptResolveRef.current?.(null);
      return new Promise<string | null>(resolve => {
        textPromptResolveRef.current = resolve;
        setTextPromptReq({ message, defaultValue, allowEmpty: opts?.allowEmpty });
      });
    },
    []
  );

  const resolveTextPrompt = useCallback((v: string | null) => {
    textPromptResolveRef.current?.(v);
    textPromptResolveRef.current = null;
    setTextPromptReq(null);
  }, []);

  return { textPromptReq, appPrompt, resolveTextPrompt };
}
