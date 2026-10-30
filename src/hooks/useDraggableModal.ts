import type React from 'react';
import { useState, useCallback, useRef, useEffect } from 'react';
import { startPointerDrag } from '../services/panelDrag';

export type ResizeDirection = 'n' | 's' | 'e' | 'w' | 'ne' | 'nw' | 'se' | 'sw';

export interface UseDraggableModalOptions {
  defaultWidth: number;
  defaultHeight: number;
  minWidth?: number;
  minHeight?: number;
}

export interface ModalGeometry {
  x: number;
  y: number;
  w: number;
  h: number;
}

const RESIZE_CURSORS: Record<ResizeDirection, string> = {
  n: 'ns-resize',
  s: 'ns-resize',
  e: 'ew-resize',
  w: 'ew-resize',
  ne: 'nesw-resize',
  nw: 'nwse-resize',
  se: 'nwse-resize',
  sw: 'nesw-resize'
};

const calcDefaultGeom = (defaultW: number, defaultH: number): ModalGeometry => {
  const w = Math.min(defaultW, Math.max(320, window.innerWidth - 32));
  const h = Math.min(defaultH, Math.max(240, window.innerHeight - 32));
  const x = Math.max(16, Math.round((window.innerWidth - w) / 2));
  const y = Math.max(16, Math.round((window.innerHeight - h) / 2));
  return { x, y, w, h };
};

export function useDraggableModal(options: UseDraggableModalOptions) {
  const { defaultWidth, defaultHeight, minWidth = 400, minHeight = 260 } = options;

  const [geom, setGeom] = useState<ModalGeometry>(() => calcDefaultGeom(defaultWidth, defaultHeight));
  const [isMaximized, setIsMaximized] = useState(false);

  const geomRef = useRef(geom);
  geomRef.current = geom;

  const isMaximizedRef = useRef(isMaximized);
  isMaximizedRef.current = isMaximized;

  const preMaxGeomRef = useRef<ModalGeometry | null>(null);

  // 视口尺寸变化时收敛坐标，避免弹窗移出屏幕
  useEffect(() => {
    const onResize = () => {
      if (isMaximizedRef.current) {
        setGeom({
          x: 16,
          y: 16,
          w: Math.max(minWidth, window.innerWidth - 32),
          h: Math.max(minHeight, window.innerHeight - 32)
        });
      } else {
        setGeom(prev => {
          const w = Math.min(prev.w, window.innerWidth - 32);
          const h = Math.min(prev.h, window.innerHeight - 32);
          const maxX = Math.max(0, window.innerWidth - 100);
          const maxY = Math.max(0, window.innerHeight - 40);
          const x = Math.max(0, Math.min(maxX, prev.x));
          const y = Math.max(0, Math.min(maxY, prev.y));
          return { x, y, w, h };
        });
      }
    };
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, [minWidth, minHeight]);

  const handleTitlePointerDown = useCallback((e: React.PointerEvent) => {
    if (isMaximizedRef.current) return;
    const target = e.target as HTMLElement;
    if (target.closest('button, input, select, textarea, a, .mac-traffic-lights')) return;

    const startX = e.clientX;
    const startY = e.clientY;
    const startGeom = { ...geomRef.current };

    startPointerDrag(e, 'default', {
      onMove: (cx, cy) => {
        const dx = cx - startX;
        const dy = cy - startY;
        const maxX = Math.max(0, window.innerWidth - 100);
        const maxY = Math.max(0, window.innerHeight - 40);
        const nextX = Math.max(0, Math.min(maxX, startGeom.x + dx));
        const nextY = Math.max(0, Math.min(maxY, startGeom.y + dy));
        setGeom(prev => ({ ...prev, x: nextX, y: nextY }));
      }
    });
  }, []);

  const handleResizePointerDown = useCallback(
    (dir: ResizeDirection, e: React.PointerEvent) => {
      if (isMaximizedRef.current) return;
      e.preventDefault();
      e.stopPropagation();

      const startX = e.clientX;
      const startY = e.clientY;
      const startGeom = { ...geomRef.current };
      const cursor = RESIZE_CURSORS[dir];

      startPointerDrag(e, cursor, {
        onMove: (cx, cy) => {
          const dx = cx - startX;
          const dy = cy - startY;
          let { x, y, w, h } = startGeom;

          if (dir.includes('e')) {
            w = Math.max(minWidth, startGeom.w + dx);
          }
          if (dir.includes('s')) {
            h = Math.max(minHeight, startGeom.h + dy);
          }
          if (dir.includes('w')) {
            const targetW = Math.max(minWidth, startGeom.w - dx);
            x = startGeom.x + (startGeom.w - targetW);
            w = targetW;
          }
          if (dir.includes('n')) {
            const targetH = Math.max(minHeight, startGeom.h - dy);
            y = startGeom.y + (startGeom.h - targetH);
            h = targetH;
          }

          setGeom({ x, y, w, h });
        }
      });
    },
    [minWidth, minHeight]
  );

  const toggleMaximize = useCallback(() => {
    if (isMaximized) {
      if (preMaxGeomRef.current) {
        setGeom(preMaxGeomRef.current);
      }
      setIsMaximized(false);
    } else {
      preMaxGeomRef.current = geomRef.current;
      setGeom({
        x: 16,
        y: 16,
        w: Math.max(minWidth, window.innerWidth - 32),
        h: Math.max(minHeight, window.innerHeight - 32)
      });
      setIsMaximized(true);
    }
  }, [isMaximized, minWidth, minHeight]);

  return {
    geom,
    isMaximized,
    handleTitlePointerDown,
    handleResizePointerDown,
    toggleMaximize
  };
}
