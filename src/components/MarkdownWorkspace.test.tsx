import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { api } from "../api";
import { MarkdownWorkspace } from "./MarkdownWorkspace";

beforeAll(() => {
  const createRect = () => new DOMRect(0, 0, 0, 0);
  if (!Range.prototype.getClientRects) {
    Range.prototype.getClientRects = () => {
      const rect = createRect();
      const rects = [rect];
      return Object.assign(rects, { item: () => rect }) as unknown as DOMRectList;
    };
  }
  if (!Range.prototype.getBoundingClientRect) {
    Range.prototype.getBoundingClientRect = createRect;
  }
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("MarkdownWorkspace drag and drop", () => {
  it("moves a dragged note into the dropped folder", async () => {
    const suffix = Date.now().toString();
    const folder = await api.createMarkdownCategory(`拖放目标-${suffix}`);
    const document = await api.createMarkdownDocument({
      title: `待移动笔记-${suffix}`,
      content: "拖放测试",
      category_id: null
    });

    render(<MarkdownWorkspace />);

    const noteButton = await screen.findByRole("button", { name: document.title });
    const folderButton = await screen.findByRole("button", { name: folder.name });
    const dataTransfer = {
      dropEffect: "none",
      effectAllowed: "none",
      setData: vi.fn()
    };

    fireEvent.dragStart(noteButton, { dataTransfer });
    fireEvent.dragOver(folderButton, { dataTransfer });
    fireEvent.drop(folderButton, { dataTransfer });

    await waitFor(async () => {
      const moved = await api.getMarkdownDocument(document.id);
      expect(moved.category_id).toBe(folder.id);
      expect(moved.category_name).toBe(folder.name);
    });
    const folderBranch = folderButton.closest(".markdownTreeBranch");
    expect(folderBranch).not.toBeNull();
    const nestedNote = within(folderBranch as HTMLElement).getByRole("button", { name: document.title });
    expect(nestedNote.style.getPropertyValue("--tree-depth")).toBe("1");
    expect(folderButton.classList.contains("dropTarget")).toBe(false);
  });

  it("renames a folder inline without opening a prompt", async () => {
    const suffix = Date.now().toString();
    const folder = await api.createMarkdownCategory(`待重命名-${suffix}`);
    const promptSpy = vi.spyOn(window, "prompt");
    const timeoutSpy = vi.spyOn(window, "setTimeout");

    render(<MarkdownWorkspace />);

    const folderButton = await screen.findByRole("button", { name: folder.name });
    fireEvent.contextMenu(folderButton);
    fireEvent.click(await screen.findByRole("button", { name: "重命名" }));

    const input = await screen.findByRole("textbox", {
      name: `重命名文件夹 ${folder.name}`
    });
    await waitFor(() => expect(document.activeElement).toBe(input));
    expect((input as HTMLInputElement).value).toBe(folder.name);

    const nextName = `已重命名-${suffix}`;
    fireEvent.change(input, { target: { value: nextName } });
    fireEvent.keyDown(input, { key: "Enter" });

    await waitFor(async () => {
      const categories = await api.listMarkdownCategories();
      expect(categories.find((item) => item.id === folder.id)?.name).toBe(nextName);
    });
    expect(await screen.findByRole("button", { name: nextName })).toBeTruthy();
    expect(promptSpy).not.toHaveBeenCalled();

    const toast = await screen.findByRole("status");
    expect(toast.textContent).toContain("文件夹已重命名");
    const dismissCall = timeoutSpy.mock.calls.find(([, delay]) => delay === 4500);
    expect(dismissCall).toBeDefined();
    act(() => {
      const callback = dismissCall?.[0];
      if (typeof callback === "function") callback();
    });
    await waitFor(() => expect(screen.queryByRole("status")).toBeNull());
  });
});
