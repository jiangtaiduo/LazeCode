# libghostty-vt（原生）

Ghostty 的 VT 引擎原生静态库。VT 解析在 Rust 进程内完成，前端只接收
紧凑的单元格帧（120x40 约 38 KB），不再经由 wasm。

## 为什么不复用 wasm

| | wasm32 | 原生 arm64 |
| :--- | ---: | ---: |
| 解析吞吐 | 9.8 MB/s | **151.5 MB/s**（15.5×） |
| 全屏快照（120x40） | 2.752 ms/帧 | **0.098 ms/帧**（28×） |
| 每格读取 | 0.573 µs | **0.02 µs** |

同一份负载（`src-tauri/src/vt.rs::engine_bench_stream`，120000 行 / 14.05 MB）。
运行 `npm run bench:terminal` 可复现原生与 xterm.js 的对照。

## 重新生成

```bash
GHOSTTY_REF=<commit-or-tag> npm run build:vt-lib
```

`GHOSTTY_REF` is required so the binary is reproducible; the script records the
commit, Zig version and SHA-256 in `build-info.txt` / `SHA256SUMS`.

**必须带 `-Doptimize=ReleaseFast`** —— 未优化的 Zig 产物解析只有 0.3 MB/s，
比优化版慢约 680 倍，且从体积（21.8 MB vs 10.5 MB）就能看出区别。

## ABI 注意事项

原生 ABI 与 wasm32 **不同**（指针 8 字节 vs 4 字节）：

| 结构体 | wasm32 | 原生 arm64 |
| :--- | :--- | :--- |
| `GhosttyGridRef` | size 12，`node@4`，`x@8` | **size 24，`node@8`，`x@16`** |
| `GhosttyGridRef.size` | u32 | **u64** |

硬编码 wasm 时期的偏移会让 `node` 读到错误的地址。所有布局以
`ghostty_type_json()` 的原生输出为准，并有 `pty::tests::native_layout_differs_from_wasm` 锁定。
