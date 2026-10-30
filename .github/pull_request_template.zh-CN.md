[英文](pull_request_template.md) | 简体中文

## 改了什么

<!-- 一两句话。如果有对应问题，请附上链接。 -->

## 验证

- [ ] `npm run check`
- [ ] `npm run test:ui`
- [ ] `npm run verify:fmt`
- [ ] `./scripts/clippy.sh`
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --locked`
- [ ] 界面改动包含截图或录屏

## 说明

- [ ] 线格式或键表改动包含对应的 `verifyWire.ts` 检查。
- [ ] 面向用户的文档已同时更新英文和简体中文两版。
- [ ] 终端基准只通过 `UPDATE_GOLDENS=1` 重新生成，并已逐项检查。
- [ ] 未包含密钥、签名密钥、真实用户数据或构建产物。
