import { useState, useRef, useCallback } from 'react';
import type { TextPromptRequest } from '../components/TextPromptDialog';

export interface UseTextPromptReturn {
  textPromptReq: TextPromptRequest | null;
  appPrompt: (
    message: string,
    defaultValue?: string,
    opts?: { allowEmpty?: boolean; checkbox?: { label: string; defaultChecked?: boolean } }
  ) => Promise<string | null>;
  resolveTextPrompt: (v: string | null, checked?: boolean) => void;
  /** 最近一次确认 时 checkbox 的勾选态（resolveTextPrompt 之后读，无 checkbox 恒 true） */
  lastChecked: boolean;
}

/**
 * 通用文本输入框（替代 window.prompt —— wry 未实现 JS 面板，prompt 直接回 null）：
 * 调用方 `await appPrompt(message, defaultValue)`，确认 resolve 输入值，
 * 取消/Esc resolve null。pending 中再调一次会作废前一次（单框语义）。
 */
export function useTextPrompt(): UseTextPromptReturn {
  const [textPromptReq, setTextPromptReq] = useState<TextPromptRequest | null>(null);
  const textPromptResolveRef = useRef<((v: string | null) => void) | null>(null);
  /** 最近一次 checkbox 勾选态（无 checkbox 的弹窗不影响调用方读值） */
  const [lastChecked, setLastChecked] = useState(true);

  const appPrompt = useCallback(
    (
      message: string,
      defaultValue?: string,
      opts?: { allowEmpty?: boolean; checkbox?: { label: string; defaultChecked?: boolean } }
    ): Promise<string | null> => {
      textPromptResolveRef.current?.(null);
      // tsconfig lib=ES2022 没有 Promise.withResolvers；stored-resolver 语义
      // （resolve 被 resolveTextPrompt 持有到弹窗收尾）本就是 executor 形态。
      return new Promise<string | null>(resolve => {
        textPromptResolveRef.current = resolve;
        setTextPromptReq({ message, defaultValue, allowEmpty: opts?.allowEmpty, checkbox: opts?.checkbox });
      });
    },
    []
  );

  const resolveTextPrompt = useCallback(
    (v: string | null, checked?: boolean) => {
      setLastChecked(checked ?? true);
      textPromptResolveRef.current?.(v);
      textPromptResolveRef.current = null;
      setTextPromptReq(null);
    },
    []
  );

  return { textPromptReq, appPrompt, resolveTextPrompt, lastChecked };
}
