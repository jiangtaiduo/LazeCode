// 自动/手动更新服务层。
//
// 封装 Tauri 2 的官方 @tauri-apps/plugin-updater 插件：
// - 检查新版本（Rust 端发起 HTTP 请求，受 Ed25519 签名保护，绝不突破 WebView CSP）
// - 下载更新包并报告百分比进度
// - 校验并就地安装
// - 优雅退出并重启（经由 restart_app Tauri 命令）
//
// 浏览器预览下静默降级，不抛错。

import { check, type Update, type DownloadEvent } from '@tauri-apps/plugin-updater';
import { isTauriEnvironment, invokeCommand } from './tauriBridge';
import { APP_VERSION } from './appVersion';

export interface UpdateInfo {
  version: string;
  currentVersion: string;
  date?: string;
  body?: string;
  rawUpdate: Update;
}

export interface DownloadProgress {
  downloadedBytes: number;
  totalBytes: number;
  percent: number;
}

/** 格式化字节为易读文本（如 "32.4 MB"） */
export function formatBytes(bytes: number): string {
  if (!bytes || bytes <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB'];
  let val = bytes;
  let idx = 0;
  while (val >= 1024 && idx < units.length - 1) {
    val /= 1024;
    idx++;
  }
  return `${val.toFixed(idx === 0 ? 0 : 1)} ${units[idx]}`;
}

/**
 * 检查新版本。
 * @returns UpdateInfo 若有可用新版本；null 表示已是最新或非桌面环境。
 */
export async function checkForAppUpdate(): Promise<UpdateInfo | null> {
  if (!isTauriEnvironment()) {
    return null;
  }

  const update = await check({ timeout: 10000 });
  if (!update) return null;

  return {
    version: update.version,
    currentVersion: update.currentVersion || APP_VERSION,
    date: update.date,
    body: update.body,
    rawUpdate: update,
  };
}

/**
 * 下载更新包并执行安装。
 *
 * @param update check 返回的 Update 实例
 * @param onProgress 进度回调函数
 */
export async function downloadAndInstallUpdate(
  update: Update,
  onProgress?: (progress: DownloadProgress) => void
): Promise<void> {
  let downloadedBytes = 0;
  let totalBytes = 0;

  await update.download((event: DownloadEvent) => {
    switch (event.event) {
      case 'Started':
        totalBytes = event.data.contentLength ?? 0;
        onProgress?.({
          downloadedBytes: 0,
          totalBytes,
          percent: 0,
        });
        break;

      case 'Progress': {
        downloadedBytes += event.data.chunkLength;
        const percent = totalBytes > 0 ? Math.min(100, Math.round((downloadedBytes / totalBytes) * 100)) : 0;
        onProgress?.({
          downloadedBytes,
          totalBytes,
          percent,
        });
        break;
      }

      case 'Finished':
        onProgress?.({
          downloadedBytes: totalBytes > 0 ? totalBytes : downloadedBytes,
          totalBytes: totalBytes > 0 ? totalBytes : downloadedBytes,
          percent: 100,
        });
        break;
    }
  });

  // 安装下载好的更新包（macOS 下完成解压替换，需重启生效）
  await update.install();
}

/**
 * 重启应用以应用更新。
 */
export async function relaunchApp(): Promise<void> {
  if (isTauriEnvironment()) {
    await invokeCommand('restart_app');
  }
}
