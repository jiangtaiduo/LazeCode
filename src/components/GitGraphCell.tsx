import React from 'react';
import { GRAPH_BREAK_GAP } from '../services/logFolding';
import type { GraphBreak, GraphLayoutRow } from '../services/logFolding';
import type { GitCommit } from '../types';

/**
 * Git 提交图的共享绘制层。
 *
 * 原先这套常量与 SVG 只存在于 GitBottomPanel；Compare Branches 的两条独立
 * 提交日志也要画同样的 DAG，复制一份会让两处轨道宽度/断线箭头逐渐漂移。
 * 这里只放纯绘制与坐标，不挂点击（WKWebView 的 SVG 子元素收不到点击，
 * 折叠箭头命中由调用方按行坐标处理）。
 */
export const LANE_W = 16;
export const GRAPH_H = 26;
export const NODE_R = 4;
/** PyCharm 风格的分支轨道配色，按轨道下标循环取色 */
export const GRAPH_PALETTE = ['#3fb950', '#a371f7', '#f0883e', '#39c5cf', '#e3b341', '#58a6ff', '#f85149', '#d2a8ff'];

export const laneX = (lane: number) => 10 + lane * LANE_W;
export const laneColor = (lane: number) => GRAPH_PALETTE[lane % GRAPH_PALETTE.length];

/** 断开箭头的命中矩形。FoldArrow 画命中区、行级坐标判定共用它，两边不会走样。 */
export const foldArrowHit = (lane: number, dir: 'up' | 'down') => {
  const x = laneX(lane);
  const down = dir === 'down';
  const tip = down ? GRAPH_H - GRAPH_BREAK_GAP : GRAPH_BREAK_GAP;
  const base = down ? tip - 6 : tip + 6;
  return {
    x0: x - 10,
    x1: x + 10,
    y0: down ? base - 4 : 0,
    y1: down ? GRAPH_H : tip + 4
  };
};

/**
 * 断开段某一端的跳转箭头。
 *
 * 这里**只负责画**，不挂 onClick：WKWebView 上 SVG 子元素的命中测试收不到
 * 点击（fill="transparent" 与 pointer-events="all" 都实测无效），挂在箭头上的
 * 处理函数永远不会执行。点击统一由行级调用方按坐标判定。
 */
const FoldArrow: React.FC<{ lane: number; dir: 'up' | 'down'; tooltip?: string }> = ({
  lane,
  dir,
  tooltip
}) => {
  const x = laneX(lane);
  const down = dir === 'down';
  const tip = down ? GRAPH_H - GRAPH_BREAK_GAP : GRAPH_BREAK_GAP;
  const base = down ? tip - 6 : tip + 6;
  return (
    <g className="dag-fold-arrow">
      <polygon points={`${x - 4.5},${base} ${x + 4.5},${base} ${x},${tip}`} fill={laneColor(lane)} />
      {tooltip && <title>{tooltip}</title>}
    </g>
  );
};

/** 按全局 DAG 布局绘制一行提交图：直通线 + 汇入曲线 + 出发曲线 + 节点。
 *  `brk` 给出本行哪些轨道处于断开段端点，见 GRAPH_BREAK_MIN_SPAN。 */
export const GraphCell: React.FC<{
  commit: GitCommit;
  layout?: GraphLayoutRow;
  brk?: GraphBreak;
}> = ({ commit, layout, brk }) => {
  const node = layout?.node ?? commit.graphTrack ?? 0;
  const hasInNode = layout?.hasInNode ?? commit.graphTrack !== undefined;
  const ins = layout?.ins ?? commit.graphIns ?? [];
  const straight = layout?.straight ?? commit.graphStraight ?? [];
  const outs = layout?.outs ?? commit.graphOuts ?? [];
  const maxLane = Math.max(node, ...ins, ...straight, ...outs.flatMap(e => [e.from, e.to]), 0);
  const width = Math.max(36, laneX(maxLane) + 12);
  const xm = laneX(node);
  const half = GRAPH_H / 2;
  const gapAbove = new Set((brk?.above ?? []).map(b => b.lane));
  const gapBelow = new Set((brk?.below ?? []).map(b => b.lane));
  const omitted = brk?.omitted;

  return (
    <svg width={width} height={GRAPH_H} className="dag-svg">
      {straight
        .filter(l => !omitted?.has(l))
        .map(l => (
          <line
            key={`s-${l}`}
            x1={laneX(l)}
            y1={gapAbove.has(l) ? GRAPH_BREAK_GAP : 0}
            x2={laneX(l)}
            y2={gapBelow.has(l) ? GRAPH_H - GRAPH_BREAK_GAP : GRAPH_H}
            stroke={laneColor(l)}
            strokeWidth={2}
          />
        ))}
      {hasInNode && (
        <line
          x1={xm}
          y1={gapAbove.has(node) ? GRAPH_BREAK_GAP : 0}
          x2={xm}
          y2={half}
          stroke={laneColor(node)}
          strokeWidth={2}
        />
      )}
      {ins
        .filter(l => l !== node)
        .map(l => (
          <path
            key={`i-${l}`}
            d={`M ${laneX(l)} 0 Q ${laneX(l)} ${half} ${xm} ${half}`}
            fill="none"
            stroke={laneColor(l)}
            strokeWidth={2}
          />
        ))}
      {outs.map((e, idx) =>
        e.to === node ? (
          <line key={`o-${idx}`} x1={xm} y1={half} x2={xm} y2={GRAPH_H} stroke={laneColor(e.to)} strokeWidth={2} />
        ) : (
          <path
            key={`o-${idx}`}
            d={`M ${xm} ${half} Q ${laneX(e.to)} ${half} ${laneX(e.to)} ${GRAPH_H}`}
            fill="none"
            stroke={laneColor(e.to)}
            strokeWidth={2}
          />
        )
      )}
      <circle cx={xm} cy={half} r={NODE_R} fill={laneColor(node)} stroke="#fff" strokeWidth={1.5} />
      {(brk?.above ?? []).map(b => (
        <FoldArrow key={`a-${b.lane}`} lane={b.lane} dir="up" tooltip={b.tooltip} />
      ))}
      {(brk?.below ?? []).map(b => (
        <FoldArrow key={`b-${b.lane}`} lane={b.lane} dir="down" tooltip={b.tooltip} />
      ))}
    </svg>
  );
};
