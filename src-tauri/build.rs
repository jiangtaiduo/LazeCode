// 链接 libghostty-vt 原生静态库
//
// 该库由 Ghostty 源码用 Zig 构建（aarch64/x86_64 原生产物，非 wasm）：
//
//   git clone --depth 1 https://github.com/ghostty-org/ghostty
//   cd ghostty && zig build -Demit-lib-vt=true
//   → zig-out/lib/libghostty-vt.a
//
// 与 wasm 版本的对比：
//   - 原生 SIMD，无 wasm 沙箱边界
//   - 解析与单元格遍历全部在 Rust 内完成，不再有「每格一次 JS↔wasm 跨界」
//
// # 静态库来源（三选一，按优先级）
//
//   1. GHOSTTY_VT_LIB_DIR=<dir>       —— 指向含 libghostty-vt.a 的目录。
//      用于系统安装的产物（包管理器 / 自行 zig build 后不 copy 进本仓库）。
//      让本仓库的 .a 可以从版本控制里移除，保持跨架构可构建。
//   2. <repo>/vendor/ghostty-vt/lib   —— 本仓库内置的 macOS arm64 产物，
//      为「clone 后零配置可跑」提供的兜底。
//   3. 两者都找不到                  —— panic，并给出可执行的修复命令。
//
// GHOSTTY_VT_LIB_DIR 为空字符串或空白时视为未设置（env::var 不会给出
// 空值的情况不存在，但用户可能显式传空），避免把空路径传给 linker。

use std::env;
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let vendored_dir = manifest.join("vendor/ghostty-vt/lib");
    let vendored_lib = vendored_dir.join("libghostty-vt.a");

    // 1) 环境变量优先：使用者自己构建 / 系统安装的库。
    let env_dir = env::var("GHOSTTY_VT_LIB_DIR")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);

    let (lib_dir, static_lib, source) = match env_dir {
        Some(dir) => {
            let lib = dir.join("libghostty-vt.a");
            if !lib.is_file() {
                panic!(
                    "GHOSTTY_VT_LIB_DIR 指向 {}，但其中没有 libghostty-vt.a\n\
                     预期文件：{}",
                    dir.display(),
                    lib.display()
                );
            }
            (dir, lib, "GHOSTTY_VT_LIB_DIR")
        }
        None if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos")
            && env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("aarch64") =>
        {
            (vendored_dir.clone(), vendored_lib.clone(), "vendor")
        }
        None => panic!(
            "本仓库内置的 libghostty-vt.a 仅适用于 macOS aarch64；当前目标 {}-{}。\n\
             请设置 GHOSTTY_VT_LIB_DIR 指向针对该目标构建的库（见 README）。",
            env::var("CARGO_CFG_TARGET_OS").unwrap_or_else(|_| "unknown".into()),
            env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_else(|_| "unknown".into()),
        ),
    };

    if !static_lib.is_file() {
        // 缺库时 rustc 只在链接阶段报 "unable to find library -lghostty-vt"，
        // 离真实原因很远。提前检查，给出两条可执行的修复路径。
        panic!(
            "缺少静态库 {}\n\
             两条修复路径（任选一）：\n\
             \n\
             A. 用本仓库源码构建 Ghostty 并把产物拷进来（需要 Zig）：\n\
                npm run build:vt-lib\n\
             \x20\x20   → src-tauri/vendor/ghostty-vt/lib/libghostty-vt.a\n\
             \n\
             B. 已自行构建/系统已安装时，指向那个目录（不 copy 进仓库）：\n\
                GHOSTTY_VT_LIB_DIR=/path/to/ghostty/zig-out/lib cargo build\n\
             \n\
             注意内置的 vendor 产物是 macOS arm64 的，其它架构/系统请走 A 或 B。",
            static_lib.display()
        );
    }

    // 只在显式要求时打印库来源：cargo 的 warning 通道在 CI 里与 lint 混在
    // 一起，看不出这条只是「用了哪个路径」。库缺失的 panic 已经足够醒目。
    if env::var_os("GHOSTTY_VT_LIB_DIR").is_some() || std::env::var_os("VERBOSE_BUILD").is_some() {
        println!(
            "cargo:warning=libghostty-vt 来源: {} ({})",
            source,
            static_lib.display()
        );
    }
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static=ghostty-vt");

    // Zig 产物需要系统 C++ 运行时与 libc++ 的少量符号；macOS 上还需 frameworks
    // build script 里的 cfg! 是宿主，不是编译目标；交叉编译会选错运行时。
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "macos" {
        println!("cargo:rustc-link-lib=c++");
        println!("cargo:rustc-link-lib=framework=CoreFoundation");
        println!("cargo:rustc-link-lib=framework=Security");
    } else if target_os == "linux" {
        println!("cargo:rustc-link-lib=stdc++");
    }

    // env 与 vendor 两处都是触发重链的条件
    println!("cargo:rerun-if-changed={}", static_lib.display());
    println!("cargo:rerun-if-env-changed=GHOSTTY_VT_LIB_DIR");

    tauri_build::build();
}
