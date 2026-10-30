[English](pull_request_template.md) | [Simplified Chinese](pull_request_template.zh-CN.md)

## What changed

<!-- One or two sentences. Link the issue if there is one. -->

## Verification

- [ ] `npm run check`
- [ ] `npm run test:ui`
- [ ] `npm run verify:fmt`
- [ ] `./scripts/clippy.sh`
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --locked`
- [ ] UI change includes a screenshot or recording

## Notes

- [ ] Wire-format or key-table changes include a matching `verifyWire.ts` check.
- [ ] User-facing documentation was updated in both English and Simplified Chinese.
- [ ] Terminal goldens were regenerated only with `UPDATE_GOLDENS=1` and reviewed.
- [ ] No secrets, signing keys, real user data or build output are included.
