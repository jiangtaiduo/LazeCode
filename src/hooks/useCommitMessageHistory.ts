// 提交信息历史（从 App.tsx 迁出，行为 + 容量不变，新增清空入口）。
//
// 只记录「真实提交过」的信息，供提交框历史下拉复用（上限 10 条）。
// 隐私说明：用户若在 message 里粘贴过 token，它会以明文躺在本地存储；
// 因此提供 `clearCommitMessages`，由提交框历史下拉的「清除历史」按钮调用。
// 存储走 historyStore（命名空间 + 版本 + 旧 key `ide_recent_commit_messages`
// 自动认领迁移），不再直写 localStorage。

import { useCallback, useState } from 'react';
import { COMMIT_MESSAGES, clearHistory, loadHistory, pushHistory } from '../services/historyStore';

export function useCommitMessageHistory(): {
  commitMessages: string[];
  setCommitMessages: React.Dispatch<React.SetStateAction<string[]>>;
  rememberCommitMessage: (msg: string) => void;
  clearCommitMessages: () => void;
} {
  const [commitMessages, setCommitMessages] = useState<string[]>(() => loadHistory(COMMIT_MESSAGES));

  const rememberCommitMessage = useCallback((msg: string) => {
    // pushHistory 自带去重顶 + 截断 + 落盘，返回的新列表直接进 state。
    // 不在 setState updater 里做写盘：updater 在 StrictMode/并发下可重跑，
    // 副作用放 updater 里等于埋双写（此处幂等所以无事，但别开先例）。
    setCommitMessages(pushHistory(COMMIT_MESSAGES, msg));
  }, []);

  const clearCommitMessages = useCallback(() => {
    clearHistory(COMMIT_MESSAGES);
    setCommitMessages([]);
  }, []);

  return { commitMessages, setCommitMessages, rememberCommitMessage, clearCommitMessages };
}

/** 启动时把落盘历史读回 state（loadWorkspace 的一部分，抽出以便单测初始化语义）。 */
export function loadCommitMessages(): string[] {
  return loadHistory(COMMIT_MESSAGES);
}
