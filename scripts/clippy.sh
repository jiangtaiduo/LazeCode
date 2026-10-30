#!/bin/sh
# Clippy 唯一入口（对齐 Zed 的 `./script/clippy`）。
#
# 本地与 CI 都走它：手敲 `cargo clippy` 容易漏 `--all-targets` /
# `-D warnings`，造成"本地绿、CI 红"的门禁漂移（见 .audit/05 审计 5）。
set -eu
cd "$(dirname "$0")/../src-tauri"
exec cargo clippy --all-targets -- -D warnings
