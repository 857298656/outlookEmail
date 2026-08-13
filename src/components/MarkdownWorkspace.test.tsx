import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { api } from "../api";
import { MarkdownWorkspace } from "./MarkdownWorkspace";

const dialogMocks = vi.hoisted(() => ({
  open: vi.fn(),
  save: vi.fn()
}));

vi.mock("@tauri-apps/plugin-dialog", () => dialogMocks);

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
  dialogMocks.open.mockReset();
  dialogMocks.save.mockReset();
  delete (window as typeof window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
});

describe("MarkdownWorkspace folder tree", () => {
  it("keeps folders collapsed on the initial load", async () => {
    const now = new Date().toISOString();
    vi.spyOn(api, "listMarkdownCategories").mockResolvedValue([
      {
        id: -2001,
        name: "父文件夹",
        parent_id: null,
        sort_order: 0,
        document_count: 0,
        created_at: now,
        updated_at: now
      },
      {
        id: -2002,
        name: "子文件夹",
        parent_id: -2001,
        sort_order: 0,
        document_count: 0,
        created_at: now,
        updated_at: now
      }
    ]);
    vi.spyOn(api, "listMarkdownDocuments").mockResolvedValue([]);

    render(<MarkdownWorkspace />);

    const parentFolder = await screen.findByRole("button", { name: "父文件夹" });
    expect(screen.queryByRole("button", { name: "子文件夹" })).toBeNull();

    fireEvent.click(parentFolder);
    expect(await screen.findByRole("button", { name: "子文件夹" })).toBeTruthy();
  });
});

describe("MarkdownWorkspace root context menu", () => {
  it("shows root actions on blank space and creates notes and folders at the root", async () => {
    const now = new Date().toISOString();
    const folderName = `根目录文件夹-${Date.now()}`;
    const createDocumentSpy = vi.spyOn(api, "createMarkdownDocument").mockResolvedValue({
      id: -1001,
      title: "根目录笔记",
      content: "",
      category_id: null,
      category_name: null,
      source_path: null,
      created_at: now,
      updated_at: now
    });
    const createCategorySpy = vi.spyOn(api, "createMarkdownCategory").mockImplementation(
      async (name, parentId) => ({
        id: -1002,
        name,
        parent_id: parentId ?? null,
        sort_order: 0,
        document_count: 0,
        created_at: now,
        updated_at: now
      })
    );
    const listDocumentsSpy = vi.spyOn(api, "listMarkdownDocuments");

    render(<MarkdownWorkspace />);

    const tree = document.querySelector<HTMLElement>(".markdownTree");
    expect(tree).not.toBeNull();
    fireEvent.contextMenu(tree as HTMLElement, { clientX: 120, clientY: 180 });

    const menu = await screen.findByLabelText("根目录操作");
    expect(within(menu).getByRole("button", { name: "新建笔记" })).toBeTruthy();
    expect(within(menu).getByRole("button", { name: "新建文件夹" })).toBeTruthy();
    expect(within(menu).getByRole("button", { name: "导入文件" })).toBeTruthy();
    expect(within(menu).getByRole("button", { name: "刷新" })).toBeTruthy();

    fireEvent.click(within(menu).getByRole("button", { name: "新建笔记" }));
    await waitFor(() =>
      expect(createDocumentSpy).toHaveBeenCalledWith(
        expect.objectContaining({ category_id: null })
      )
    );

    fireEvent.contextMenu(tree as HTMLElement, { clientX: 140, clientY: 200 });
    fireEvent.click(
      within(await screen.findByLabelText("根目录操作")).getByRole("button", {
        name: "新建文件夹"
      })
    );
    const input = await screen.findByRole("textbox", { name: "新文件夹名称" });
    fireEvent.change(input, { target: { value: folderName } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(createCategorySpy).toHaveBeenCalledWith(folderName, null));

    const listCallsBeforeRefresh = listDocumentsSpy.mock.calls.length;
    fireEvent.contextMenu(tree as HTMLElement, { clientX: 160, clientY: 220 });
    fireEvent.click(
      within(await screen.findByLabelText("根目录操作")).getByRole("button", {
        name: "刷新"
      })
    );
    await waitFor(() =>
      expect(listDocumentsSpy.mock.calls.length).toBeGreaterThan(listCallsBeforeRefresh)
    );
  });

  it("imports a file from the blank-space menu into the root", async () => {
    const now = new Date().toISOString();
    const path = "C:\\notes\\root-note.txt";
    const readSpy = vi.spyOn(api, "readMarkdownFile").mockResolvedValue({
      path,
      file_name: "root-note.txt",
      content: "root content",
      size: 12
    });
    const createSpy = vi.spyOn(api, "createMarkdownDocument").mockResolvedValue({
      id: Date.now(),
      title: "root-note",
      content: "root content",
      category_id: null,
      category_name: null,
      source_path: null,
      created_at: now,
      updated_at: now
    });
    dialogMocks.open.mockResolvedValue(path);

    render(<MarkdownWorkspace />);
    await waitFor(() => expect(document.querySelector(".markdownTree")).not.toBeNull());
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {}
    });

    const tree = document.querySelector<HTMLElement>(".markdownTree") as HTMLElement;
    fireEvent.contextMenu(tree, { clientX: 120, clientY: 180 });
    fireEvent.click(
      within(await screen.findByLabelText("根目录操作")).getByRole("button", {
        name: "导入文件"
      })
    );

    await waitFor(() => expect(readSpy).toHaveBeenCalledWith(path));
    expect(createSpy).toHaveBeenCalledWith({
      title: "root-note",
      content: "root content",
      category_id: null,
      source_path: null
    });
  });
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
    expect(toast.classList.contains("success")).toBe(true);
    expect(toast.classList.contains("error")).toBe(false);
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
