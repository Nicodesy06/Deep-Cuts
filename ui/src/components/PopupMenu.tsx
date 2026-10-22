// A context menu that stays inside the window: placed at the pointer, then
// moved up/left by its measured size when it would run off an edge.
import { useLayoutEffect, useRef, useState, type ReactNode } from "react";

export function PopupMenu({ x, y, children }: { x: number; y: number; children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ left: x, top: y });
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const m = 8;
    const { width, height } = el.getBoundingClientRect();
    setPos({
      left: Math.max(m, Math.min(x, window.innerWidth - width - m)),
      top: Math.max(m, Math.min(y, window.innerHeight - height - m)),
    });
  }, [x, y]);
  return (
    <div ref={ref} className="menu" style={pos} role="menu">
      {children}
    </div>
  );
}
