import { useRef, type KeyboardEvent, type PointerEvent, type Ref } from "react";
import { cx } from "./cx";

/** Limits of a size a handle sets, in px at 100 % text size. */
export interface SizeLimits {
  min: number;
  max: number;
  default: number;
  /** What one arrow key press changes. */
  step: number;
}

export function clampSize(value: number, limits: Pick<SizeLimits, "min" | "max">): number {
  return Math.round(Math.min(limits.max, Math.max(limits.min, value)));
}

/** Sizes are in px at 100 % text size; the page grows with the text (rem). */
export function textScale(): number {
  return Number.parseFloat(getComputedStyle(document.documentElement).fontSize) / 16 || 1;
}

interface ResizeHandleProps {
  /**
   * WAI-ARIA window splitter: "vertical" is a line between two columns, moved sideways (← and →);
   * "horizontal" one between two rows, moved up and down (↑ and ↓). The value is the size of the
   * part before the line (the column on the left, the row on top).
   */
  orientation: "vertical" | "horizontal";
  /** The size shown right now. */
  value: number;
  /** `max` may be less than the stored limit where the window leaves less room (Home, End, arrows). */
  limits: SizeLimits;
  onChange: (value: number) => void;
  /** Enter, the splitter's own key (the sidebar folds with it); nothing when absent. */
  onEnter?: () => void;
  label: string;
  valueText: string;
  /** Id of the part whose size this sets. */
  controls: string;
  /** Where the handle lies: over the edge of a column, or between two siblings. */
  className: string;
  ref?: Ref<HTMLDivElement>;
  "data-testid": string;
}

/**
 * The drag handle between two parts of the window (stage 8 for the sidebar, stage 18 also between
 * the list and the reading pane): 8 px to grab, straddling the hairline, a line in the accent
 * while hovered or dragged. Pointer capture while dragging, arrow keys by `step`, Home and End to
 * the limits, a double click back to the default. The size is in px at 100 % text size; a drag
 * divides the pointer's way by the text scale.
 */
export function ResizeHandle({ orientation, value, limits, onChange, onEnter, label, valueText, controls, className, ref, ...rest }: ResizeHandleProps) {
  const drag = useRef<{ from: number; value: number; scale: number } | null>(null);
  const vertical = orientation === "vertical";
  const set = (next: number) => {
    onChange(clampSize(next, limits));
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const grow = vertical ? "ArrowRight" : "ArrowDown";
    const shrink = vertical ? "ArrowLeft" : "ArrowUp";
    const next =
      event.key === grow
        ? value + limits.step
        : event.key === shrink
          ? value - limits.step
          : event.key === "Home"
            ? limits.min
            : event.key === "End"
              ? limits.max
              : null;
    if (next !== null) {
      event.preventDefault();
      set(next);
    } else if (event.key === "Enter" && onEnter) {
      event.preventDefault();
      onEnter();
    }
  };
  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = { from: vertical ? event.clientX : event.clientY, value, scale: textScale() };
  };
  const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    const start = drag.current;
    if (start) set(start.value + ((vertical ? event.clientX : event.clientY) - start.from) / start.scale);
  };
  const stop = () => {
    drag.current = null;
  };
  return (
    <div
      ref={ref}
      role="separator"
      aria-orientation={orientation}
      aria-label={label}
      aria-controls={controls}
      aria-valuemin={limits.min}
      aria-valuemax={limits.max}
      aria-valuenow={value}
      aria-valuetext={valueText}
      tabIndex={0}
      onKeyDown={onKeyDown}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={stop}
      onPointerCancel={stop}
      onDoubleClick={() => {
        set(limits.default);
      }}
      data-testid={rest["data-testid"]}
      className={cx(
        "z-20 touch-none after:absolute after:transition-colors hover:after:bg-accent-fill active:after:bg-accent-fill",
        vertical
          ? "w-2 cursor-col-resize after:inset-y-0 after:start-[calc(var(--ui-space-1)-1px)] after:w-[2px]"
          : "h-2 cursor-row-resize after:inset-x-0 after:top-[calc(var(--ui-space-1)-1px)] after:h-[2px]",
        className,
      )}
    />
  );
}
