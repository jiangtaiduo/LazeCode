// 隐藏终端画布的释放策略（纯逻辑，供组件与单测共用）。
//
// 只释放「非活动分屏 + 页面已隐藏」的画布：
// - 前台切终端 tab 不释放，保证点击即显示，不出现整屏重绘；
// - 活动分屏永不释放，切回应用第一帧仍是离开前的画面；
// - 页面隐藏满 5 分钟后才释放后台分屏，切回后首次点开它们才付一次
//   全量快照 + 整屏绘制（预算 ~1 帧，见 App 的定位/性能验收）。
export const HIDDEN_CANVAS_RELEASE_MS = 5 * 60 * 1000;

export function shouldReleaseCanvas(onScreen: boolean, pageHidden: boolean): boolean {
  return !onScreen && pageHidden;
}
