import React from 'react';
import type { ResizeDirection } from '../hooks/useDraggableModal';

export const ModalResizeHandles: React.FC<{
  onResizeStart: (dir: ResizeDirection, e: React.PointerEvent) => void;
}> = ({ onResizeStart }) => {
  const dirs: ResizeDirection[] = ['n', 's', 'e', 'w', 'ne', 'nw', 'se', 'sw'];
  return (
    <>
      {dirs.map(d => (
        <div
          key={d}
          className={`modal-resize-handle modal-resize-${d}`}
          onPointerDown={e => onResizeStart(d, e)}
        />
      ))}
    </>
  );
};
