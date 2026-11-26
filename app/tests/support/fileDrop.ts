// Stage 23 (decision 0082): files dragged onto a window from the Finder or the Explorer, as the
// page sees them (jsdom has no DataTransfer): the type "Files", the files and their items.
import { fireEvent } from "@testing-library/react";

/** A drag of `files` (and `folders`, which have no bytes) from outside the page. */
export function fileTransfer(files: File[], folders: string[] = []) {
  const entries = [
    ...files.map((file) => ({ file, directory: false })),
    ...folders.map((name) => ({ file: new File([], name), directory: true })),
  ];
  return {
    types: ["Files"],
    effectAllowed: "all",
    dropEffect: "none",
    files: entries.map((entry) => entry.file),
    items: entries.map((entry) => ({
      kind: "file",
      type: entry.file.type,
      getAsFile: () => entry.file,
      webkitGetAsEntry: () => ({ isDirectory: entry.directory, isFile: !entry.directory, name: entry.file.name }),
    })),
    getData: () => "",
    setData: () => undefined,
  };
}

/** The pointer brings files over `target` (the window's own listeners see it first). */
export function dragFilesOver(target: Element, transfer: ReturnType<typeof fileTransfer>) {
  fireEvent.dragEnter(target, { dataTransfer: transfer });
  return fireEvent.dragOver(target, { dataTransfer: transfer });
}

/** The files are let go over `target`. Answers whether the page took the drop (default prevented). */
export function dropFiles(target: Element, transfer: ReturnType<typeof fileTransfer>) {
  return !fireEvent.drop(target, { dataTransfer: transfer });
}
