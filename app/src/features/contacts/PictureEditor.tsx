import { Minus, Plus } from "lucide-react";
import { useId, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import type { ContactPictureSource, PictureCrop } from "../../services/ipc";

/** Most zoom: the square is an eighth of the image's shorter side. */
const MAX_ZOOM = 8;
/** Arrow keys move by this share of the square, with Shift by four times as much. */
const KEY_STEP = 0.05;
/** One press of plus or minus. */
const ZOOM_STEP = 1.25;

/** The largest square of the image, in the middle. */
export function initialCrop(width: number, height: number): PictureCrop {
  const size = Math.min(width, height);
  return { x: Math.floor((width - size) / 2), y: Math.floor((height - size) / 2), size };
}

/** A square of at least one pixel inside the image. */
export function clampCrop(crop: PictureCrop, width: number, height: number): PictureCrop {
  const largest = Math.min(width, height);
  const size = Math.round(Math.min(largest, Math.max(Math.max(1, largest / MAX_ZOOM), crop.size)));
  return {
    x: Math.round(Math.min(width - size, Math.max(0, crop.x))),
    y: Math.round(Math.min(height - size, Math.max(0, crop.y))),
    size,
  };
}

/** The square at zoom `zoom` (1: the whole shorter side) around the same centre. */
export function zoomCrop(crop: PictureCrop, zoom: number, width: number, height: number): PictureCrop {
  const size = Math.min(width, height) / Math.min(MAX_ZOOM, Math.max(1, zoom));
  const centreX = crop.x + crop.size / 2;
  const centreY = crop.y + crop.size / 2;
  return clampCrop({ x: centreX - size / 2, y: centreY - size / 2, size }, width, height);
}

interface PictureEditorProps {
  source: ContactPictureSource;
  /** The contact's name, for the labels. */
  name: string;
  saving: boolean;
  onSave: (crop: PictureCrop) => void;
  onCancel: () => void;
}

/**
 * Stage 17 (decision 0050): cut a square out of the chosen image. The square stays in place and
 * the image moves under it (drag, or arrow keys on the focused square), zoom with the slider or
 * plus and minus; the round mask shows the picture as avatars show it. The image is the one the
 * core's helper encoded for this (never the file's own bytes); the square is sent back in its
 * pixels and the helper cuts and encodes it again. Escape cancels.
 */
export function PictureEditor({ source, name, saving, onSave, onCancel }: PictureEditorProps) {
  const { t } = useTranslation();
  const hintId = useId();
  const zoomId = useId();
  const { width, height } = source;
  const [crop, setCrop] = useState(() => initialCrop(width, height));
  const frame = useRef<HTMLDivElement>(null);
  const drag = useRef<{ x: number; y: number; crop: PictureCrop } | null>(null);
  const zoom = Math.min(width, height) / crop.size;

  const move = (dx: number, dy: number) => {
    setCrop((current) => clampCrop({ ...current, x: current.x + dx, y: current.y + dy }, width, height));
  };
  const zoomBy = (factor: number) => {
    setCrop((current) => zoomCrop(current, (Math.min(width, height) / current.size) * factor, width, height));
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const step = crop.size * KEY_STEP * (event.shiftKey ? 4 : 1);
    const moves: Record<string, [number, number]> = {
      ArrowLeft: [-step, 0],
      ArrowRight: [step, 0],
      ArrowUp: [0, -step],
      ArrowDown: [0, step],
    };
    const delta = moves[event.key];
    if (delta) {
      event.preventDefault();
      move(delta[0], delta[1]);
    } else if (event.key === "+" || event.key === "=") {
      event.preventDefault();
      zoomBy(ZOOM_STEP);
    } else if (event.key === "-" || event.key === "_") {
      event.preventDefault();
      zoomBy(1 / ZOOM_STEP);
    } else if (event.key === "Enter") {
      event.preventDefault();
      if (!saving) onSave(crop);
    }
  };

  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = { x: event.clientX, y: event.clientY, crop };
  };
  const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    const start = drag.current;
    const box = frame.current?.getBoundingClientRect();
    if (!start || !box || box.width === 0) return;
    // Pixels of the screen to pixels of the image: the frame shows `crop.size` image pixels.
    const scale = start.crop.size / box.width;
    setCrop(
      clampCrop(
        {
          ...start.crop,
          x: start.crop.x - (event.clientX - start.x) * scale,
          y: start.crop.y - (event.clientY - start.y) * scale,
        },
        width,
        height,
      ),
    );
  };
  const endDrag = () => {
    drag.current = null;
  };

  return (
    <div
      className="flex flex-col gap-4"
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          event.stopPropagation();
          onCancel();
        }
      }}
      data-testid="picture-editor"
    >
      <div className="flex flex-col gap-1">
        <h3 className="text-14 font-semibold text-primary">{t("contacts.view.cropper.title")}</h3>
        <p id={hintId} className="text-13 text-secondary">
          {t("contacts.view.cropper.hint")}
        </p>
      </div>
      <div
        ref={frame}
        role="group"
        tabIndex={0}
        aria-label={t("contacts.view.cropper.area", { name })}
        aria-describedby={hintId}
        onKeyDown={onKeyDown}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
        className="relative size-(--ui-cropper-size) cursor-grab touch-none overflow-hidden rounded-card bg-canvas select-none active:cursor-grabbing"
        data-testid="picture-crop-area"
        data-crop={`${String(crop.x)},${String(crop.y)},${String(crop.size)}`}
      >
        {/* The view box is the square: the frame shows exactly the pixels that become the picture.
            Attributes, not inline styles (CSP). */}
        <svg
          aria-hidden
          viewBox={`${String(crop.x)} ${String(crop.y)} ${String(crop.size)} ${String(crop.size)}`}
          preserveAspectRatio="xMidYMid slice"
          className="pointer-events-none absolute inset-0 size-full"
        >
          <image href={source.dataUrl} x={0} y={0} width={width} height={height} />
        </svg>
        {/* The round avatar the picture becomes; outside it the image is dimmed. */}
        <div aria-hidden className="pointer-events-none absolute inset-0 rounded-full shadow-[0_0_0_100vmax_var(--ui-cropper-mask)] ring-2 ring-surface-raised" />
      </div>
      <div className="flex w-(--ui-cropper-size) items-center gap-2">
        <button
          type="button"
          onClick={() => {
            zoomBy(1 / ZOOM_STEP);
          }}
          aria-label={t("contacts.view.cropper.zoomOut")}
          className="flex size-8 shrink-0 items-center justify-center rounded-control text-secondary hover:bg-hover hover:text-primary"
          data-testid="picture-zoom-out"
        >
          <Minus aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
        </button>
        <label htmlFor={zoomId} className="sr-only">
          {t("contacts.view.cropper.zoom")}
        </label>
        <input
          id={zoomId}
          type="range"
          min={1}
          max={MAX_ZOOM}
          step={0.05}
          value={zoom}
          onChange={(event) => {
            setCrop((current) => zoomCrop(current, Number(event.target.value), width, height));
          }}
          className="min-w-0 flex-1 accent-accent-fill"
          data-testid="picture-zoom"
        />
        <button
          type="button"
          onClick={() => {
            zoomBy(ZOOM_STEP);
          }}
          aria-label={t("contacts.view.cropper.zoomIn")}
          className="flex size-8 shrink-0 items-center justify-center rounded-control text-secondary hover:bg-hover hover:text-primary"
          data-testid="picture-zoom-in"
        >
          <Plus aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
        </button>
      </div>
      <div className="flex flex-wrap gap-2">
        <Button
          disabled={saving}
          onClick={() => {
            onSave(crop);
          }}
          data-testid="picture-save"
        >
          {saving ? t("contacts.view.cropper.saving") : t("contacts.view.cropper.save")}
        </Button>
        <Button variant="secondary" disabled={saving} onClick={onCancel} data-testid="picture-cancel">
          {t("contacts.view.cropper.cancel")}
        </Button>
      </div>
    </div>
  );
}
