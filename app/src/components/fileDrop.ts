import { useEffect, useEffectEvent, useState } from "react";
import { CoreError } from "../services/ipc";

/**
 * Stage 23 (decision 0082): files dragged onto a window from the Finder or the Explorer. The
 * shell no longer takes drops (its handler kept every drag inside the page from working), so the
 * page receives them as HTML drag events with `File`s: names and bytes, never paths.
 *
 * One owner per page: listeners on the window in the capture phase, before any element (the
 * rich text editor would otherwise insert a file's name or address as text). The last mounted
 * receiver (`useWindowFileDrop`: the composer, the signature editor) takes the files; without
 * one, a file dropped anywhere does nothing (the web view would otherwise open it). Drags that
 * started in the page (an account, mail rows, an image in the editor) are never treated as files,
 * even where the engine lists "Files" among their types.
 */

/** What a receiver gets: the files, and the names of folders it cannot take. */
export interface DroppedFiles {
  files: File[];
  folders: string[];
}

interface Receiver {
  receive: (dropped: DroppedFiles) => void;
  setOver: (over: boolean) => void;
}

const receivers: Receiver[] = [];
/** A drag started in this page and has not ended. */
let internalDrag = false;
let installed: Window | null = null;
let overTimer: number | undefined;
/** `dragover` keeps coming while the pointer is over the window; without it the hint goes. */
const OVER_TIMEOUT_MS = 300;

/** Whether a drag over the window carries files from outside the page. */
export function carriesFiles(event: DragEvent): boolean {
  const types = event.dataTransfer?.types;
  return !internalDrag && types !== undefined && Array.from(types).includes("Files");
}

/** The dropped entries, folders apart (they have no bytes the page could read). */
export function droppedFiles(transfer: DataTransfer): DroppedFiles {
  const files: File[] = [];
  const folders: string[] = [];
  const items = Array.from(transfer.items as ArrayLike<DataTransferItem> | undefined ?? []);
  const fromItems = items.filter((item) => item.kind === "file");
  if (fromItems.length > 0) {
    for (const item of fromItems) {
      const file = item.getAsFile();
      if (!file) continue;
      const entry = typeof item.webkitGetAsEntry === "function" ? item.webkitGetAsEntry() : null;
      if (entry?.isDirectory) folders.push(file.name);
      else files.push(file);
    }
    return { files, folders };
  }
  return { files: Array.from(transfer.files), folders };
}

/** What a receiver shows for dropped folders: like a path that is no file (`invalid_argument`). */
export function folderError(folders: readonly string[]): CoreError {
  return new CoreError({ code: "invalid_argument", message: `${folders.join(", ")}: a folder is not a file`, detail: null });
}

function current(): Receiver | undefined {
  return receivers.at(-1);
}

function leave() {
  window.clearTimeout(overTimer);
  overTimer = undefined;
  for (const receiver of receivers) receiver.setOver(false);
}

function over(event: DragEvent) {
  if (!carriesFiles(event)) return;
  // Always handled here: a drop is either taken by a receiver or does nothing.
  event.preventDefault();
  event.stopPropagation();
  const receiver = current();
  if (event.dataTransfer) event.dataTransfer.dropEffect = receiver ? "copy" : "none";
  if (!receiver) return;
  receiver.setOver(true);
  window.clearTimeout(overTimer);
  overTimer = window.setTimeout(leave, OVER_TIMEOUT_MS);
}

function drop(event: DragEvent) {
  const files = carriesFiles(event);
  internalDrag = false;
  if (!files) return;
  event.preventDefault();
  event.stopPropagation();
  leave();
  const receiver = current();
  if (receiver && event.dataTransfer) receiver.receive(droppedFiles(event.dataTransfer));
}

function dragLeave(event: DragEvent) {
  // Out of the window: no element is entered any more.
  if (carriesFiles(event) && event.relatedTarget === null && (event.clientX <= 0 || event.clientY <= 0 || event.clientX >= window.innerWidth || event.clientY >= window.innerHeight)) leave();
}

const started = () => {
  internalDrag = true;
};
const ended = () => {
  internalDrag = false;
};

/**
 * Installs the page's file drop handling once (main window, composer and message windows). Call
 * it before the first render.
 */
export function installFileDrop(target: Window = window): void {
  if (installed === target) return;
  installed = target;
  target.addEventListener("dragstart", started, true);
  target.addEventListener("dragend", ended, true);
  // A new press after a drag whose source vanished (no `dragend`) starts clean.
  target.addEventListener("mousedown", ended, true);
  target.addEventListener("dragenter", over, true);
  target.addEventListener("dragover", over, true);
  target.addEventListener("dragleave", dragLeave, true);
  target.addEventListener("drop", drop, true);
}

/** Removes the handling again (tests). */
export function uninstallFileDrop(): void {
  const target = installed;
  if (!target) return;
  installed = null;
  target.removeEventListener("dragstart", started, true);
  target.removeEventListener("dragend", ended, true);
  target.removeEventListener("mousedown", ended, true);
  target.removeEventListener("dragenter", over, true);
  target.removeEventListener("dragover", over, true);
  target.removeEventListener("dragleave", dragLeave, true);
  target.removeEventListener("drop", drop, true);
  internalDrag = false;
  leave();
}

/**
 * Files dropped anywhere on the window go to `onFiles` while the calling component is mounted
 * (and the last one that asked). `over`: files are being dragged over the window right now.
 */
export function useWindowFileDrop(onFiles: (dropped: DroppedFiles) => void): { over: boolean } {
  const [isOver, setOver] = useState(false);
  const receive = useEffectEvent(onFiles);
  useEffect(() => {
    installFileDrop();
    const receiver: Receiver = {
      receive: (dropped) => {
        receive(dropped);
      },
      setOver,
    };
    receivers.push(receiver);
    return () => {
      const index = receivers.indexOf(receiver);
      if (index >= 0) receivers.splice(index, 1);
    };
  }, []);
  return { over: isOver };
}
