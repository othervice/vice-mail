// Stage 23 (decision 0082): files dragged onto a window reach the page as HTML `File`s (the
// shell's drop handler is off). Without a receiver a drop does nothing; drags that started in the
// page are never taken as files; the bytes go to the core in pieces, and a failure discards what
// was begun.
import { fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { carriesFiles, droppedFiles, installFileDrop, uninstallFileDrop } from "../src/components/fileDrop";
import { composeIpc } from "../src/services/ipc";
import { useComposerStore } from "../src/stores/composer";
import { ipcError, makeAccount, mockCore, standardMailboxes } from "./support/core";
import { dragFilesOver, dropFiles, fileTransfer } from "./support/fileDrop";
import { renderApp } from "./support/render";

const ADA = makeAccount({ writesEnabled: true });

afterEach(() => {
  uninstallFileDrop();
  useComposerStore.setState({ session: null });
});

describe("files dragged onto a window", () => {
  it("do nothing where nothing takes them, and the web view does not open them", () => {
    installFileDrop();
    const target = document.createElement("div");
    document.body.append(target);
    const transfer = fileTransfer([new File(["x"], "notes.txt")]);
    // Over the page: refused (no copy cursor), but handled, so the web view does not open the file.
    expect(dragFilesOver(target, transfer)).toBe(false);
    expect(transfer.dropEffect).toBe("none");
    expect(dropFiles(target, transfer)).toBe(true);
    target.remove();
  });

  it("are told apart from drags that started in the page, and folders from files", () => {
    installFileDrop();
    const source = document.createElement("div");
    document.body.append(source);
    const transfer = fileTransfer([new File(["x"], "photo.png")]);
    const over = (target: Element) => {
      const event = new Event("dragover", { bubbles: true, cancelable: true });
      Object.defineProperty(event, "dataTransfer", { value: transfer });
      target.dispatchEvent(event);
      return event as DragEvent;
    };
    expect(carriesFiles(over(source))).toBe(true);
    // An image dragged inside the page may list "Files" too: not a file from outside.
    fireEvent.dragStart(source);
    expect(carriesFiles(over(source))).toBe(false);
    fireEvent.dragEnd(source);
    expect(carriesFiles(over(source))).toBe(true);
    // A press after a drag whose source vanished starts clean.
    fireEvent.dragStart(source);
    fireEvent.mouseDown(source);
    expect(carriesFiles(over(source))).toBe(true);
    source.remove();

    const split = droppedFiles(fileTransfer([new File(["a"], "a.pdf")], ["Belege"]) as unknown as DataTransfer);
    expect(split.files.map((file) => file.name)).toEqual(["a.pdf"]);
    expect(split.folders).toEqual(["Belege"]);
  });

  it("a failed piece discards the uploads begun so far, and the error reaches the composer", async () => {
    const core = mockCore({
      accounts: [ADA],
      mailboxes: standardMailboxes(),
      failures: { append_file_upload: ipcError("internal") },
    });
    const { user } = renderApp();
    await user.click(await screen.findByRole("button", { name: "New message" }));
    await screen.findByLabelText("Subject");
    const composer = screen.getByTestId("composer");
    const transfer = fileTransfer([new File(["one"], "one.txt"), new File(["two"], "two.txt")]);
    expect(dropFiles(composer, transfer)).toBe(true);
    expect(await screen.findByTestId("composer-action-error")).toBeTruthy();
    await waitFor(() => {
      expect(core.callsOf("discard_file_upload")).toEqual([{ uploadId: "upload-1" }]);
    });
    expect(core.callsOf("add_uploaded_attachments")).toEqual([]);
    expect(core.compose.uploads.size).toBe(0);
  });

  it("an empty file goes over without a piece", async () => {
    const core = mockCore({ accounts: [ADA], mailboxes: standardMailboxes() });
    const draft = await composeIpc.createDraft({ kind: "new", accountId: ADA.id, identityId: null, to: [] });
    const saved = await composeIpc.attachFiles(draft.id, [new File([], "empty.txt")]);
    expect(saved.attachments.map((a) => a.fileName)).toEqual(["empty.txt"]);
    expect(core.callsOf("append_file_upload")).toEqual([]);
  });
});
