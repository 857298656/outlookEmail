import {
  ChevronDown,
  ChevronRight,
  ChevronsUpDown,
  CircleHelp,
  Copy,
  FileCode2,
  FileDown,
  FileImage,
  FilePlus2,
  FileText,
  Folder,
  FolderDown,
  FolderOpen,
  FolderPlus,
  Info,
  Loader2,
  MoreHorizontal,
  PanelLeftClose,
  PanelLeftOpen,
  Pencil,
  RefreshCw,
  Save,
  Search,
  Trash2,
  Upload,
  X
} from "lucide-react";
import { open, save } from "@tauri-apps/plugin-dialog";
import type { Crepe as CrepeInstance } from "@milkdown/crepe";
import { imageBlockSchema } from "@milkdown/kit/component/image-block";
import {
  commandsCtx,
  editorViewCtx,
  remarkStringifyOptionsCtx
} from "@milkdown/kit/core";
import type { CmdKey } from "@milkdown/kit/core";
import type { Ctx } from "@milkdown/kit/ctx";
import {
  addBlockTypeCommand,
  blockquoteSchema,
  bulletListSchema,
  codeBlockSchema,
  headingSchema,
  hrSchema,
  inlineCodeSchema,
  linkSchema,
  listItemSchema,
  orderedListSchema,
  paragraphSchema,
  selectTextNearPosCommand,
  setBlockTypeCommand,
  toggleEmphasisCommand,
  toggleInlineCodeCommand,
  toggleStrongCommand,
  wrapInBlockTypeCommand
} from "@milkdown/kit/preset/commonmark";
import {
  addColAfterCommand,
  addColBeforeCommand,
  addRowAfterCommand,
  addRowBeforeCommand,
  createTable,
  strikethroughSchema,
  tableSchema,
  toggleStrikethroughCommand
} from "@milkdown/kit/preset/gfm";
import { lift, toggleMark } from "@milkdown/kit/prose/commands";
import { keymap } from "@milkdown/kit/prose/keymap";
import type { Mark } from "@milkdown/kit/prose/model";
import { liftListItem, wrapInList } from "@milkdown/kit/prose/schema-list";
import { NodeSelection, Plugin, PluginKey, TextSelection } from "@milkdown/kit/prose/state";
import { Decoration, DecorationSet, type EditorView } from "@milkdown/kit/prose/view";
import { $markSchema, $prose, $remark } from "@milkdown/kit/utils";
import "@milkdown/crepe/theme/common/style.css";
import "@milkdown/crepe/theme/classic.css";
import {
  type CSSProperties,
  type DragEvent as ReactDragEvent,
  type MouseEvent as ReactMouseEvent,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState
} from "react";
import { api } from "../api";
import {
  cleanMarkdownEditorHtml,
  standaloneMarkdownHtml
} from "../lib/markdownExport";
import {
  formatMarkdownImageRequest,
  parseMarkdownImageRequest
} from "../lib/markdownImage";
import {
  MARKDOWN_FILE_EXTENSIONS,
  MARKDOWN_IMPORT_EXTENSIONS,
  markdownImportTitle
} from "../lib/markdownImport";
import { calculateMarkdownPdfLayout } from "../lib/markdownPdf";
import { MarkdownSaveTracker } from "../lib/markdownSaveTracker";
import { MarkdownWriteQueue } from "../lib/markdownWriteQueue";
import {
  deleteMarkdownTable,
  deleteMarkdownTableColumn,
  deleteMarkdownTableRow,
  getMarkdownTableToolbarState,
  insertMarkdownTableHeaderRowBefore,
  setMarkdownTableColumnAlignment
} from "../lib/markdownTable";
import type { MarkdownCategory, MarkdownDocument } from "../types";

type SaveState = "idle" | "dirty" | "saving" | "saved" | "error";
type FolderMenu = { category: MarkdownCategory; x: number; y: number };
type DocumentMenu = { document: MarkdownDocument; x: number; y: number };
type RootMenu = { x: number; y: number };
type FolderDraft = { parentId: number | null; name: string };
type FolderRenameDraft = { categoryId: number; name: string };
type ActiveMarkdownDocument = {
  id: number | undefined;
  title: string;
  content: string;
  categoryId: number | null;
  sourcePath: string | null;
  dirty: boolean;
};
type InlineLinkEditState = {
  from: number;
  to: number;
  href: string;
  token: string;
  autofocus: boolean;
};
export type MarkdownLeaveHandlers = {
  save: () => Promise<boolean>;
  discard: () => void;
};

type MarkdownWorkspaceProps = {
  onDirtyChange?: (dirty: boolean) => void;
  onRegisterLeaveHandlers?: (handlers: MarkdownLeaveHandlers | null) => void;
};

const MAX_IMAGE_BYTES = 8 * 1024 * 1024;
const TOOLBAR_TOOLTIPS = [
  "插入代码块 Ctrl+Shift+K",
  "插入表格 Ctrl+T",
  "插入图片",
  "有序列表 Ctrl+Shift+[",
  "无序列表 Ctrl+Shift+]",
  "任务列表 Ctrl+Shift+O",
  "引用 Ctrl+Shift+Q",
  "一级标题 Ctrl+1",
  "二级标题 Ctrl+2",
  "三级标题 Ctrl+3",
  "分割线",
  "超链接 Ctrl+K",
  "公式块（换行 Shift+Enter）",
  "内联公式",
  "加粗 Ctrl+B",
  "斜体 Ctrl+L",
  "下划线 Ctrl+U",
  "内联代码 Ctrl+`",
  "删除线 Ctrl+~",
  "高亮 Ctrl+="
];

type MarkdownAstNode = {
  type: string;
  value?: string;
  children?: MarkdownAstNode[];
};

const toolbarIcon = (content: string, extraClass = "") => `
  <svg class="${extraClass}" xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"
    fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
    ${content}
  </svg>
`;

const headingOneIcon = toolbarIcon('<path d="M5 6v12M12 6v12M5 12h7"/><path d="M17 10h2v8m-2 0h4"/>', "markdownStrokeIcon");
const headingTwoIcon = toolbarIcon('<path d="M4 6v12M11 6v12M4 12h7"/><path d="M16 12c0-1.3 1-2 2.2-2 1.1 0 2 .7 2 1.8 0 1.8-4.2 3.2-4.2 6.2h4.5"/>', "markdownStrokeIcon");
const headingThreeIcon = toolbarIcon('<path d="M4 6v12M11 6v12M4 12h7"/><path d="M16 10h4l-2.4 3 1.2.1c1.2.2 1.8 1 1.8 2.2 0 1.6-1.2 2.7-3 2.7-1 0-1.8-.3-2.4-.8"/>', "markdownStrokeIcon");
const inlineMathIcon = toolbarIcon('<path d="M5 17 10 7m0 10L5 7"/><path d="M14 8c.2-1.3 1.1-2 2.3-2 1.1 0 2 .7 2 1.7 0 1.5-3.8 2.7-3.8 5.3h4.2"/>', "markdownStrokeIcon");
const formulaBlockIcon = toolbarIcon('<path d="M18 5H8l5 7-5 7h10"/><path d="M5 5h2M5 19h2"/>', "markdownStrokeIcon");
const underlineIcon = toolbarIcon('<path d="M6 4v6a6 6 0 0 0 12 0V4M4 20h16"/>', "markdownStrokeIcon");
const highlightIcon = toolbarIcon('<path d="m9 11-4 4 4 4 9-9-4-4-5 5Z"/><path d="m14 6 4-2 2 2-2 4M3 21h18"/>', "markdownStrokeIcon");
const insertTableIcon = toolbarIcon('<rect x="4" y="4" width="16" height="16" rx="1"/><path d="M4 10h16M4 15h16M10 4v16M15 4v16"/>', "markdownStrokeIcon");
const tableRowBeforeIcon = toolbarIcon('<path d="M5 5h14M12 19V8m-4 4 4-4 4 4"/>');
const tableRowAfterIcon = toolbarIcon('<path d="M5 19h14M12 5v11m-4-4 4 4 4-4"/>');
const tableDeleteRowIcon = toolbarIcon('<path d="M5 12h14"/>');
const tableColBeforeIcon = toolbarIcon('<path d="M5 5v14M19 12H8m4-4-4 4 4 4"/>');
const tableColAfterIcon = toolbarIcon('<path d="M19 5v14M5 12h11m-4-4 4 4-4 4"/>');
const tableDeleteColIcon = toolbarIcon('<circle cx="12" cy="12" r="8"/><path d="M8 12h8"/>');
const tableAlignLeftIcon = toolbarIcon('<path d="M5 7h14M5 12h10M5 17h14"/>');
const tableAlignCenterIcon = toolbarIcon('<path d="M5 7h14M7 12h10M5 17h14"/>');
const tableAlignRightIcon = toolbarIcon('<path d="M5 7h14M9 12h10M5 17h14"/>');
const tableDeleteIcon = toolbarIcon('<rect x="5" y="6" width="14" height="12" rx="1"/><path d="m9 10 6 4m0-4-6 4"/>');
const inlineLinkEditKey = new PluginKey<InlineLinkEditState | null>("markdown-inline-link-edit");

const underlineSchema = $markSchema("markdownUnderline", () => ({
  parseDOM: [{ tag: "u" }, { style: "text-decoration=underline" }],
  toDOM: () => ["u", 0],
  parseMarkdown: {
    match: (node) => node.type === "markdownUnderline",
    runner: (state, node, markType) => {
      state.openMark(markType);
      state.next(node.children);
      state.closeMark(markType);
    }
  },
  toMarkdown: {
    match: (mark) => mark.type.name === "markdownUnderline",
    runner: (state, mark) => {
      state.withMark(mark, "markdownUnderline");
    }
  }
}));

const highlightSchema = $markSchema("markdownHighlight", () => ({
  parseDOM: [{ tag: "mark" }],
  toDOM: () => ["mark", 0],
  parseMarkdown: {
    match: (node) => node.type === "markdownHighlight",
    runner: (state, node, markType) => {
      state.openMark(markType);
      state.next(node.children);
      state.closeMark(markType);
    }
  },
  toMarkdown: {
    match: (mark) => mark.type.name === "markdownHighlight",
    runner: (state, mark) => {
      state.withMark(mark, "markdownHighlight");
    }
  }
}));

const markdownTableSchema = tableSchema.extendSchema((previous) => (ctx) => ({
  ...previous(ctx),
  // A valid GFM table can contain only its header row. Supporting that shape
  // lets the row toolbar delete the final body row without faking a blank row.
  content: "table_header_row table_row*"
}));

function transformHtmlMarks(node: MarkdownAstNode) {
  if (!node.children) return;
  node.children.forEach(transformHtmlMarks);
  const pairs = [
    { open: "<u>", close: "</u>", type: "markdownUnderline" },
    { open: "<mark>", close: "</mark>", type: "markdownHighlight" }
  ];
  pairs.forEach(({ open, close, type }) => {
    const next: MarkdownAstNode[] = [];
    for (let index = 0; index < node.children!.length; index += 1) {
      const child = node.children![index]!;
      if (child.type !== "html" || child.value?.trim().toLocaleLowerCase() !== open) {
        next.push(child);
        continue;
      }
      const closingIndex = node.children!.findIndex(
        (candidate, candidateIndex) =>
          candidateIndex > index &&
          candidate.type === "html" &&
          candidate.value?.trim().toLocaleLowerCase() === close
      );
      if (closingIndex < 0) {
        next.push(child);
        continue;
      }
      const children = node.children!.slice(index + 1, closingIndex);
      const markNode = { type, children };
      transformHtmlMarks(markNode);
      next.push(markNode);
      index = closingIndex;
    }
    node.children = next;
  });
}

const markdownCustomMarksRemark = $remark(
  "markdownCustomMarks",
  () => () => (tree: MarkdownAstNode) => {
    transformHtmlMarks(tree);
  }
);

function isMarkActive(ctx: Ctx, markName: string) {
  const view = ctx.get(editorViewCtx);
  const markType = view.state.schema.marks[markName];
  if (!markType) return false;
  const { from, to, empty, $from } = view.state.selection;
  if (!empty) return view.state.doc.rangeHasMark(from, to, markType);
  return Boolean(
    view.state.storedMarks?.some((mark) => mark.type === markType) ||
    $from.marks().some((mark) => mark.type === markType)
  );
}

function toggleSchemaMark(ctx: Ctx, markName: string) {
  const view = ctx.get(editorViewCtx);
  const markType = view.state.schema.marks[markName];
  if (!markType) return false;
  if (view.state.selection.empty) {
    const transaction = isMarkActive(ctx, markName)
      ? view.state.tr.removeStoredMark(markType)
      : view.state.tr.addStoredMark(markType.create());
    view.dispatch(transaction);
    view.focus();
    return true;
  }
  const handled = toggleMark(markType)(view.state, view.dispatch, view);
  view.focus();
  return handled;
}

function callCommand<T>(ctx: Ctx, command: { key: CmdKey<T> }, payload?: T) {
  return ctx.get(commandsCtx).call(command.key, payload);
}

function linkRangeAtSelection(ctx: Ctx) {
  const view = ctx.get(editorViewCtx);
  const { doc, selection } = view.state;
  const markType = linkSchema.type(ctx);
  const selectedMark =
    selection.$from.marks().find((mark) => mark.type === markType) ??
    selection.$to.marks().find((mark) => mark.type === markType);

  if (!selectedMark) {
    if (!selection.empty && doc.rangeHasMark(selection.from, selection.to, markType)) {
      let mark: Mark | undefined;
      doc.nodesBetween(selection.from, selection.to, (node) => {
        mark ??= node.marks.find((candidate) => candidate.type === markType);
        return !mark;
      });
      return mark ? { from: selection.from, to: selection.to, mark } : null;
    }
    return null;
  }

  const parentStart = selection.$from.start();
  let from = selection.from;
  let to = selection.to;
  selection.$from.parent.forEach((node, offset) => {
    if (!node.isText || !selectedMark.isInSet(node.marks)) return;
    const nodeFrom = parentStart + offset;
    const nodeTo = nodeFrom + node.nodeSize;
    if (nodeFrom <= selection.from && nodeTo >= selection.from) {
      from = nodeFrom;
      to = nodeTo;
    }
  });
  return { from, to, mark: selectedMark };
}

function isLinkActive(ctx: Ctx) {
  const view = ctx.get(editorViewCtx);
  const markType = linkSchema.type(ctx);
  const { selection } = view.state;
  return (
    Boolean(linkRangeAtSelection(ctx)) ||
    (!selection.empty && view.state.doc.rangeHasMark(selection.from, selection.to, markType))
  );
}

function toggleInlineLink(ctx: Ctx) {
  const view = ctx.get(editorViewCtx);
  const markType = linkSchema.type(ctx);
  const existing = linkRangeAtSelection(ctx);

  if (existing) {
    view.dispatch(
      view.state.tr
        .removeMark(existing.from, existing.to, markType)
        .setMeta(inlineLinkEditKey, null)
        .scrollIntoView()
    );
    view.focus();
    return true;
  }

  const { from, to, empty } = view.state.selection;
  if (empty) return false;
  const href = "https://";
  const token = `${Date.now()}-${from}-${to}`;
  view.dispatch(
    view.state.tr
      .addMark(from, to, markType.create({ href, title: null }))
      .setMeta(inlineLinkEditKey, { from, to, href, token, autofocus: true } satisfies InlineLinkEditState)
      .scrollIntoView()
  );
  return true;
}

const markdownInlineLinkEditor = $prose((ctx) => {
  const markType = linkSchema.type(ctx);

  const findLinkRangeAtPos = (view: EditorView, pos: number) => {
    const resolved = view.state.doc.resolve(pos);
    const parentStart = resolved.start();
    const linkedNodes: Array<{ from: number; to: number; mark: Mark }> = [];
    resolved.parent.forEach((node, offset) => {
      if (!node.isText) return;
      const mark = node.marks.find((candidate) => candidate.type === markType);
      if (!mark) return;
      const from = parentStart + offset;
      linkedNodes.push({ from, to: from + node.nodeSize, mark });
    });
    const index = linkedNodes.findIndex(({ from, to }) => from <= pos && pos <= to);
    if (index < 0) return null;
    const selected = linkedNodes[index]!;
    let from = selected.from;
    let to = selected.to;
    for (let cursor = index - 1; cursor >= 0; cursor -= 1) {
      const previous = linkedNodes[cursor]!;
      if (previous.to !== from || !previous.mark.eq(selected.mark)) break;
      from = previous.from;
    }
    for (let cursor = index + 1; cursor < linkedNodes.length; cursor += 1) {
      const next = linkedNodes[cursor]!;
      if (next.from !== to || !next.mark.eq(selected.mark)) break;
      to = next.to;
    }
    return { from, to, mark: selected.mark };
  };

  const finishEditing = (editorView: EditorView, href: string, keepLink: boolean) => {
    const active = inlineLinkEditKey.getState(editorView.state);
    if (!active) return;
    const nextHref = href.trim();
    let transaction = editorView.state.tr
      .removeMark(active.from, active.to, markType)
      .setMeta(inlineLinkEditKey, null);
    if (keepLink && nextHref) {
      transaction = transaction.addMark(
        active.from,
        active.to,
        markType.create({ href: nextHref, title: null })
      );
    }
    transaction = transaction.setSelection(
      TextSelection.near(transaction.doc.resolve(Math.min(active.to, transaction.doc.content.size)), 1)
    );
    editorView.dispatch(transaction.scrollIntoView());
    queueMicrotask(() => editorView.focus());
  };

  return new Plugin<InlineLinkEditState | null>({
    key: inlineLinkEditKey,
    state: {
      init: () => null,
      apply: (transaction, current) => {
        const meta = transaction.getMeta(inlineLinkEditKey) as InlineLinkEditState | null | undefined;
        if (meta !== undefined) return meta;
        if (!current || !transaction.docChanged) return current;
        const from = transaction.mapping.map(current.from, 1);
        const to = transaction.mapping.map(current.to, -1);
        return from < to ? { ...current, from, to, autofocus: false } : null;
      }
    },
    props: {
      handleClick: (view, pos, event) => {
        const target = event.target instanceof Element ? event.target.closest("a[href]") : null;
        if (!target) return false;
        const range = findLinkRangeAtPos(view, pos);
        if (!range) return false;
        event.preventDefault();
        view.dispatch(
          view.state.tr.setMeta(inlineLinkEditKey, {
            from: range.from,
            to: range.to,
            href: String(range.mark.attrs.href ?? "https://"),
            token: `${Date.now()}-${range.from}-${range.to}`,
            autofocus: true
          } satisfies InlineLinkEditState)
        );
        return true;
      },
      decorations: (state) => {
        const active = inlineLinkEditKey.getState(state);
        if (!active || active.from >= active.to) return DecorationSet.empty;
        const opening = Decoration.widget(
          active.from,
          () => {
            const span = document.createElement("span");
            span.className = "markdownInlineLinkSyntax";
            span.textContent = "[";
            return span;
          },
          { key: `inline-link-open-${active.token}`, side: -1 }
        );
        const editor = Decoration.widget(
          active.to,
          (view) => {
            const wrapper = document.createElement("span");
            wrapper.className = "markdownInlineLinkEditor";
            wrapper.contentEditable = "false";
            wrapper.append("](");

            const input = document.createElement("input");
            input.type = "text";
            input.value = active.href;
            input.className = "markdownInlineLinkInput";
            input.setAttribute("aria-label", "超链接地址");
            input.style.width = `${Math.max(8, input.value.length)}ch`;
            input.addEventListener("input", () => {
              input.style.width = `${Math.max(8, input.value.length)}ch`;
            });
            input.addEventListener("keydown", (event) => {
              event.stopPropagation();
              if (event.key === "Enter") {
                event.preventDefault();
                finishEditing(view, input.value, true);
              } else if (event.key === "Escape") {
                event.preventDefault();
                finishEditing(view, input.value, false);
              }
            });
            input.addEventListener("blur", () => finishEditing(view, input.value, true), {
              once: true
            });
            wrapper.append(input, ")");

            if (active.autofocus) {
              requestAnimationFrame(() => {
                input.focus();
                input.select();
              });
            }
            return wrapper;
          },
          { key: `inline-link-editor-${active.token}`, side: 1 }
        );
        return DecorationSet.create(state.doc, [opening, editor]);
      }
    }
  });
});

const markdownImageSourceEditor = $prose((ctx) => {
  const imageType = imageBlockSchema.type(ctx);

  return new Plugin({
    props: {
      decorations: (state) => {
        const { selection } = state;
        if (!(selection instanceof NodeSelection) || selection.node.type !== imageType) {
          return DecorationSet.empty;
        }

        const imagePosition = selection.from;
        const editor = Decoration.widget(
          imagePosition,
          (view, getPosition) => {
            const wrapper = document.createElement("div");
            wrapper.className = "markdownImageSourceEditor";
            wrapper.contentEditable = "false";

            const input = document.createElement("input");
            input.type = "text";
            input.className = "markdownImageSourceInput";
            input.value = formatMarkdownImageRequest(
              String(selection.node.attrs.caption ?? ""),
              String(selection.node.attrs.src ?? "")
            );
            input.setAttribute("aria-label", "图片请求路径");
            input.setAttribute("autocomplete", "off");
            input.spellcheck = false;

            const stopPointerEvent = (event: Event) => event.stopPropagation();
            input.addEventListener("pointerdown", stopPointerEvent);
            input.addEventListener("mousedown", stopPointerEvent);
            input.addEventListener("click", stopPointerEvent);
            input.addEventListener("input", () => {
              const position = getPosition();
              if (position === undefined) return;
              const node = view.state.doc.nodeAt(position);
              if (!node || node.type !== imageType) return;
              const request = parseMarkdownImageRequest(
                input.value,
                String(node.attrs.caption ?? "")
              );
              view.dispatch(
                view.state.tr.setNodeMarkup(position, undefined, {
                  ...node.attrs,
                  caption: request.caption,
                  src: request.src
                })
              );
            });
            input.addEventListener("keydown", (event) => {
              event.stopPropagation();
              if (event.key === "Enter") {
                event.preventDefault();
                input.blur();
              } else if (event.key === "Escape") {
                event.preventDefault();
                const position = getPosition();
                const node = position === undefined ? null : view.state.doc.nodeAt(position);
                if (node?.type === imageType) {
                  input.value = formatMarkdownImageRequest(
                    String(node.attrs.caption ?? ""),
                    String(node.attrs.src ?? "")
                  );
                }
                input.blur();
              }
            });

            wrapper.append(input);
            return wrapper;
          },
          {
            key: `markdown-image-source-${imagePosition}`,
            side: -1
          }
        );
        return DecorationSet.create(state.doc, [editor]);
      }
    }
  });
});

const markdownTableToolbar = $prose((ctx) =>
  new Plugin({
    props: {
      decorations: (state) => {
        const toolbarState = getMarkdownTableToolbarState(state);
        if (!toolbarState) return DecorationSet.empty;

        const toolbar = Decoration.widget(
          toolbarState.tablePos,
          (view) => {
            const element = document.createElement("div");
            element.className = "markdownTableToolbar";
            element.contentEditable = "false";
            element.setAttribute("role", "toolbar");
            element.setAttribute("aria-label", "表格操作");

            const addButton = (
              title: string,
              icon: string,
              action: (editorView: EditorView) => boolean | void,
              options: {
                active?: boolean;
                disabled?: boolean;
                disabledTitle?: string;
              } = {}
            ) => {
              const button = document.createElement("button");
              button.type = "button";
              button.title =
                options.disabled && options.disabledTitle ? options.disabledTitle : title;
              button.setAttribute("aria-label", title);
              button.disabled = Boolean(options.disabled);
              if (options.active !== undefined) {
                button.setAttribute("aria-pressed", String(options.active));
                if (options.active) button.classList.add("active");
              }
              button.innerHTML = icon;
              button.addEventListener("mousedown", (event) => {
                if (event.button !== 0) return;
                event.preventDefault();
                event.stopPropagation();
              });
              button.addEventListener("click", (event) => {
                event.preventDefault();
                event.stopPropagation();
                action(view);
                requestAnimationFrame(() => view.focus());
              });
              element.append(button);
            };

            const addDivider = () => {
              const divider = document.createElement("span");
              divider.className = "markdownTableToolbarDivider";
              divider.setAttribute("aria-hidden", "true");
              element.append(divider);
            };

            addButton(
              "在上方插入行",
              tableRowBeforeIcon,
              (editorView) =>
                toolbarState.rowFrom === 0
                  ? insertMarkdownTableHeaderRowBefore(
                      editorView.state,
                      editorView.dispatch,
                      editorView
                    )
                  : callCommand(ctx, addRowBeforeCommand)
            );
            addButton("在下方插入行", tableRowAfterIcon, () => callCommand(ctx, addRowAfterCommand));
            addButton(
              "删除当前行",
              tableDeleteRowIcon,
              (editorView) =>
                deleteMarkdownTableRow(editorView.state, editorView.dispatch, editorView),
              {
                disabled: !toolbarState.canDeleteRow,
                disabledTitle: "当前选区不能作为一行删除"
              }
            );
            addDivider();
            addButton("在左侧插入列", tableColBeforeIcon, () => callCommand(ctx, addColBeforeCommand));
            addButton("在右侧插入列", tableColAfterIcon, () => callCommand(ctx, addColAfterCommand));
            addButton(
              "删除当前列",
              tableDeleteColIcon,
              (editorView) =>
                deleteMarkdownTableColumn(editorView.state, editorView.dispatch, editorView),
              {
                disabled: !toolbarState.canDeleteColumn,
                disabledTitle: "表格至少需要保留一列"
              }
            );
            addDivider();
            addButton(
              "左对齐",
              tableAlignLeftIcon,
              (editorView) =>
                setMarkdownTableColumnAlignment("left")(
                  editorView.state,
                  editorView.dispatch,
                  editorView
                ),
              { active: toolbarState.alignment === "left" }
            );
            addButton(
              "居中对齐",
              tableAlignCenterIcon,
              (editorView) =>
                setMarkdownTableColumnAlignment("center")(
                  editorView.state,
                  editorView.dispatch,
                  editorView
                ),
              { active: toolbarState.alignment === "center" }
            );
            addButton(
              "右对齐",
              tableAlignRightIcon,
              (editorView) =>
                setMarkdownTableColumnAlignment("right")(
                  editorView.state,
                  editorView.dispatch,
                  editorView
                ),
              { active: toolbarState.alignment === "right" }
            );
            addDivider();
            addButton("删除表格", tableDeleteIcon, (editorView) =>
              deleteMarkdownTable(editorView.state, editorView.dispatch, editorView)
            );

            return element;
          },
          {
            key: [
              "markdown-table-toolbar",
              toolbarState.tablePos,
              toolbarState.rowFrom,
              toolbarState.rowTo,
              toolbarState.columnFrom,
              toolbarState.columnTo,
              toolbarState.rowCount,
              toolbarState.columnCount,
              toolbarState.alignment ?? "mixed"
            ].join("-"),
            side: -1
          }
        );

        return DecorationSet.create(state.doc, [toolbar]);
      }
    }
  })
);

function setHeadingLevel(ctx: Ctx, level: 1 | 2 | 3) {
  return callCommand(ctx, setBlockTypeCommand, {
    nodeType: headingSchema.type(ctx),
    attrs: { level }
  });
}

function toggleHeadingLevel(ctx: Ctx, level: 1 | 2 | 3) {
  if (isHeadingLevelActive(ctx, level)) {
    return callCommand(ctx, setBlockTypeCommand, { nodeType: paragraphSchema.type(ctx) });
  }
  return setHeadingLevel(ctx, level);
}

function isHeadingLevelActive(ctx: Ctx, level: 1 | 2 | 3) {
  const view = ctx.get(editorViewCtx);
  const { $from } = view.state.selection;
  return $from.parent.type === headingSchema.type(ctx) && $from.parent.attrs.level === level;
}

function findAncestor(ctx: Ctx, nodeName: string) {
  const { $from } = ctx.get(editorViewCtx).state.selection;
  for (let depth = $from.depth; depth > 0; depth -= 1) {
    const node = $from.node(depth);
    if (node.type.name === nodeName) {
      return { node, depth, pos: $from.before(depth) };
    }
  }
  return null;
}

function isListActive(ctx: Ctx, kind: "ordered" | "bullet" | "task") {
  const item = findAncestor(ctx, listItemSchema.type(ctx).name);
  const checked = item?.node.attrs.checked;
  if (kind === "task") return checked !== null && checked !== undefined;
  if (kind === "ordered") return Boolean(findAncestor(ctx, orderedListSchema.type(ctx).name));
  return Boolean(findAncestor(ctx, bulletListSchema.type(ctx).name)) && checked == null;
}

function updateCurrentListItemChecked(ctx: Ctx, checked: boolean | null) {
  const view = ctx.get(editorViewCtx);
  const item = findAncestor(ctx, listItemSchema.type(ctx).name);
  if (!item) return false;
  view.dispatch(
    view.state.tr.setNodeMarkup(item.pos, undefined, {
      ...item.node.attrs,
      checked
    })
  );
  view.focus();
  return true;
}

function convertCurrentList(ctx: Ctx, target: "ordered" | "bullet") {
  const view = ctx.get(editorViewCtx);
  const ordered = findAncestor(ctx, orderedListSchema.type(ctx).name);
  const bullet = findAncestor(ctx, bulletListSchema.type(ctx).name);
  const current = ordered ?? bullet;
  if (!current) return false;
  const nodeType = target === "ordered" ? orderedListSchema.type(ctx) : bulletListSchema.type(ctx);
  const attrs = target === "ordered" ? { order: 1, spread: false } : { spread: false };
  view.dispatch(view.state.tr.setNodeMarkup(current.pos, nodeType, attrs));
  view.focus();
  return true;
}

function toggleList(ctx: Ctx, kind: "ordered" | "bullet" | "task") {
  const view = ctx.get(editorViewCtx);
  const listItemType = listItemSchema.type(ctx);

  if (isListActive(ctx, kind)) {
    const handled = liftListItem(listItemType)(view.state, view.dispatch, view);
    view.focus();
    return handled;
  }

  if (kind === "task") {
    if (findAncestor(ctx, orderedListSchema.type(ctx).name)) {
      convertCurrentList(ctx, "bullet");
    } else if (!findAncestor(ctx, bulletListSchema.type(ctx).name)) {
      wrapInList(bulletListSchema.type(ctx))(view.state, view.dispatch, view);
    }
    return updateCurrentListItemChecked(ctx, false);
  }

  if (kind === "ordered") {
    updateCurrentListItemChecked(ctx, null);
    if (findAncestor(ctx, bulletListSchema.type(ctx).name)) return convertCurrentList(ctx, "ordered");
    const handled = wrapInList(orderedListSchema.type(ctx))(view.state, view.dispatch, view);
    view.focus();
    return handled;
  }

  if (findAncestor(ctx, bulletListSchema.type(ctx).name)) {
    return updateCurrentListItemChecked(ctx, null);
  }
  if (findAncestor(ctx, orderedListSchema.type(ctx).name)) {
    updateCurrentListItemChecked(ctx, null);
    return convertCurrentList(ctx, "bullet");
  }
  const handled = wrapInList(bulletListSchema.type(ctx))(view.state, view.dispatch, view);
  view.focus();
  return handled;
}

function isQuoteActive(ctx: Ctx) {
  return Boolean(findAncestor(ctx, blockquoteSchema.type(ctx).name));
}

function toggleQuote(ctx: Ctx) {
  const view = ctx.get(editorViewCtx);
  if (isQuoteActive(ctx)) {
    const handled = lift(view.state, view.dispatch);
    view.focus();
    return handled;
  }
  return wrapQuote(ctx);
}

function insertCodeBlock(ctx: Ctx) {
  return callCommand(ctx, setBlockTypeCommand, { nodeType: codeBlockSchema.type(ctx) });
}

function insertTable(ctx: Ctx) {
  if (findAncestor(ctx, "table")) return false;
  const view = ctx.get(editorViewCtx);
  const from = view.state.selection.from;
  callCommand(ctx, addBlockTypeCommand, { nodeType: createTable(ctx, 2, 2) });
  return callCommand(ctx, selectTextNearPosCommand, { pos: from });
}

function chooseImageFile(
  onSelect: (file: File) => void,
  onCancel: () => void
) {
  const input = document.createElement("input");
  input.type = "file";
  input.accept = "image/*";
  input.hidden = true;
  document.body.append(input);

  let settled = false;
  const dispose = () => input.remove();
  const settle = (callback: () => void) => {
    if (settled) return;
    settled = true;
    dispose();
    callback();
  };
  input.addEventListener(
    "change",
    () => {
      const file = input.files?.[0];
      settle(() => {
        if (file) onSelect(file);
        else onCancel();
      });
    },
    { once: true }
  );
  input.addEventListener(
    "cancel",
    () => settle(onCancel),
    { once: true }
  );
  input.click();
}

function chooseAndInsertImage(ctx: Ctx, onError: (message: string) => void) {
  const view = ctx.get(editorViewCtx);
  const bookmark = view.state.selection.getBookmark();

  chooseImageFile(
    (file) => {
      void fileToDataUrl(file)
        .then((src) => {
          const imageType = imageBlockSchema.type(ctx);
          let transaction = view.state.tr;
          try {
            transaction = transaction.setSelection(bookmark.resolve(transaction.doc));
          } catch {
            // If the document changed while the native picker was open, use its current selection.
          }

          const insertionFrom = transaction.selection.from;
          transaction = transaction.replaceSelectionWith(
            imageType.create({
              src,
              caption: file.name,
              ratio: 1
            })
          );

          let imagePosition: number | undefined;
          let nearestDistance = Number.POSITIVE_INFINITY;
          transaction.doc.descendants((node, position) => {
            if (node.type !== imageType || node.attrs.src !== src) return;
            const distance = Math.abs(position - insertionFrom);
            if (distance < nearestDistance) {
              imagePosition = position;
              nearestDistance = distance;
            }
          });
          if (imagePosition !== undefined) {
            transaction = transaction.setSelection(
              NodeSelection.create(transaction.doc, imagePosition)
            );
          }

          view.dispatch(transaction.scrollIntoView());
          view.focus();
        })
        .catch((error) => {
          onError(readError(error));
          view.focus();
        });
    },
    () => view.focus()
  );
}

function wrapList(ctx: Ctx, ordered: boolean) {
  return callCommand(ctx, wrapInBlockTypeCommand, {
    nodeType: ordered ? orderedListSchema.type(ctx) : bulletListSchema.type(ctx)
  });
}

function wrapTaskList(ctx: Ctx) {
  return callCommand(ctx, wrapInBlockTypeCommand, {
    nodeType: listItemSchema.type(ctx),
    attrs: { checked: false }
  });
}

function wrapQuote(ctx: Ctx) {
  return callCommand(ctx, wrapInBlockTypeCommand, { nodeType: blockquoteSchema.type(ctx) });
}

function insertInlineMath(ctx: Ctx) {
  const view = ctx.get(editorViewCtx);
  const { state } = view;
  const mathType = state.schema.nodes.math_inline;
  if (!mathType) return false;
  const { selection } = state;
  if (selection instanceof NodeSelection && selection.node.type === mathType) {
    const value = String(selection.node.attrs.value ?? "");
    const text = value || "公式";
    const transaction = state.tr.replaceSelectionWith(state.schema.text(text));
    transaction.setSelection(TextSelection.create(transaction.doc, selection.from, selection.from + text.length));
    view.dispatch(transaction.scrollIntoView());
    view.focus();
    return true;
  }
  const value = state.doc.textBetween(selection.from, selection.to, " ");
  const transaction = state.tr.replaceSelectionWith(mathType.create({ value }));
  view.dispatch(
    transaction
      .setSelection(NodeSelection.create(transaction.doc, selection.from))
      .scrollIntoView()
  );
  view.focus();
  return true;
}

function insertFormulaBlock(ctx: Ctx) {
  return callCommand(ctx, addBlockTypeCommand, {
    nodeType: codeBlockSchema.type(ctx),
    attrs: { language: "LaTeX" }
  });
}

function isFormulaBlockActive(ctx: Ctx) {
  const { $from } = ctx.get(editorViewCtx).state.selection;
  return (
    $from.parent.type === codeBlockSchema.type(ctx) &&
    String($from.parent.attrs.language ?? "").toLocaleLowerCase() === "latex"
  );
}

function toggleFormulaBlock(ctx: Ctx) {
  if (isFormulaBlockActive(ctx)) {
    return callCommand(ctx, setBlockTypeCommand, { nodeType: paragraphSchema.type(ctx) });
  }
  return callCommand(ctx, setBlockTypeCommand, {
    nodeType: codeBlockSchema.type(ctx),
    attrs: { language: "LaTeX" }
  });
}

const markdownToolbarShortcuts = $prose((ctx) =>
  keymap({
    "Mod-Shift-k": () => insertCodeBlock(ctx),
    "Mod-t": () => insertTable(ctx),
    "Mod-Shift-[": () => toggleList(ctx, "ordered"),
    "Mod-Shift-]": () => toggleList(ctx, "bullet"),
    "Mod-Shift-o": () => toggleList(ctx, "task"),
    "Mod-Shift-q": () => toggleQuote(ctx),
    "Mod-1": () => toggleHeadingLevel(ctx, 1),
    "Mod-2": () => toggleHeadingLevel(ctx, 2),
    "Mod-3": () => toggleHeadingLevel(ctx, 3),
    "Mod-k": () => toggleInlineLink(ctx),
    "Mod-b": () => callCommand(ctx, toggleStrongCommand),
    "Mod-l": () => callCommand(ctx, toggleEmphasisCommand),
    "Mod-u": () => toggleSchemaMark(ctx, "markdownUnderline"),
    "Mod-`": () => callCommand(ctx, toggleInlineCodeCommand),
    "Mod-~": () => callCommand(ctx, toggleStrikethroughCommand),
    "Mod-=": () => toggleSchemaMark(ctx, "markdownHighlight")
  })
);

function isTauriRuntime() {
  return "__TAURI_INTERNALS__" in window;
}

function readError(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function defaultDocumentTitle() {
  const now = new Date();
  return `新建笔记(${now.getFullYear()}/${now.getMonth() + 1}/${now.getDate()})`;
}

function fileTitle(fileName: string) {
  return markdownImportTitle(fileName) || defaultDocumentTitle();
}

function safeFileName(value: string) {
  return (
    value
      .replace(/[<>:"/\\|?*\u0000-\u001f]/g, "-")
      .replace(/[.\s]+$/g, "")
      .trim() || "Markdown 笔记"
  );
}

function formatDate(value: string) {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString("zh-CN");
}

function applyToolbarTooltips(root: HTMLElement) {
  root.querySelectorAll<HTMLButtonElement>(".milkdown-top-bar .top-bar-item").forEach((button, index) => {
    const label = TOOLBAR_TOOLTIPS[index];
    if (!label) return;
    button.dataset.tooltip = label;
    button.setAttribute("aria-label", label);
  });
}

function fileToDataUrl(file: File) {
  if (!file.type.startsWith("image/")) {
    return Promise.reject(new Error("只能插入图片文件"));
  }
  if (file.size > MAX_IMAGE_BYTES) {
    return Promise.reject(new Error("单张图片不能超过 8 MB"));
  }
  return new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(reader.error ?? new Error("读取图片失败"));
    reader.readAsDataURL(file);
  });
}

function markdownTextFallbackHtml(markdown: string) {
  const pre = document.createElement("pre");
  pre.textContent = markdown;
  return pre.outerHTML;
}

function createMarkdownExportSnapshot(rendered: HTMLElement) {
  const root = document.createElement("div");
  const milkdown = document.createElement("div");
  const content = rendered.cloneNode(false) as HTMLElement;
  root.className = "markdownCrepeRoot markdownExportSnapshot";
  milkdown.className = "milkdown";
  content.className = "ProseMirror";
  content.innerHTML = cleanMarkdownEditorHtml(rendered);
  content.removeAttribute("contenteditable");
  content.style.minHeight = "0";
  const width = Math.max(640, Math.ceil(rendered.getBoundingClientRect().width), rendered.scrollWidth);
  root.style.position = "fixed";
  root.style.left = "-100000px";
  root.style.top = "0";
  root.style.width = `${width}px`;
  root.style.pointerEvents = "none";
  milkdown.append(content);
  root.append(milkdown);
  document.body.append(root);
  return { root, content };
}

function CrepeEditor({
  documentId,
  markdown,
  onChange,
  onError
}: {
  documentId: number;
  markdown: string;
  onChange: (markdown: string) => void;
  onError: (message: string) => void;
}) {
  const rootRef = useRef<HTMLDivElement>(null);
  const onChangeRef = useRef(onChange);
  const onErrorRef = useRef(onError);

  useEffect(() => {
    onChangeRef.current = onChange;
    onErrorRef.current = onError;
  }, [onChange, onError]);

  useEffect(() => {
    if (!rootRef.current) return;
    const editorRoot = rootRef.current;
    const setImageLoadFailed = (event: Event, failed: boolean) => {
      const target = event.target;
      if (!(target instanceof HTMLImageElement)) return;
      const imageBlock = target.closest(".milkdown-image-block");
      imageBlock?.classList.toggle("markdownImageLoadFailed", failed);
    };
    const handleImageLoad = (event: Event) => setImageLoadFailed(event, false);
    const handleImageError = (event: Event) => setImageLoadFailed(event, true);
    editorRoot.addEventListener("load", handleImageLoad, true);
    editorRoot.addEventListener("error", handleImageError, true);

    let disposed = false;
    let ready = false;
    let toolbarObserver: MutationObserver | undefined;
    let crepeInstance: CrepeInstance | undefined;
    const creation = import("@milkdown/crepe").then(({ Crepe }) => {
      if (disposed || !rootRef.current) return;
      const crepe = new Crepe({
      root: rootRef.current,
      defaultValue: markdown,
      features: {
        [Crepe.Feature.TopBar]: true,
        [Crepe.Feature.LinkTooltip]: false,
        [Crepe.Feature.Table]: false
      },
      featureConfigs: {
        [Crepe.Feature.Placeholder]: {
          text: "开始输入内容，或粘贴、拖入图片……",
          mode: "doc"
        },
        [Crepe.Feature.ImageBlock]: {
          onUpload: fileToDataUrl,
          blockUploadButton: "选择图片",
          blockUploadPlaceholderText: "粘贴图片链接，或拖放 / 粘贴图片",
          blockCaptionPlaceholderText: "图片说明"
        },
        [Crepe.Feature.Toolbar]: {
          buildToolbar: (builder) => {
            const groups = builder.build();
            builder.clear();
            groups.forEach((group) => {
              const nextGroup = builder.addGroup(group.key, group.label);
              group.items.forEach((item) => {
                nextGroup.addItem(
                  item.key,
                  item.key === "link"
                    ? {
                        ...item,
                        active: isLinkActive,
                        onRun: (ctx: Ctx) => {
                          toggleInlineLink(ctx);
                        }
                      }
                    : item
                );
              });
            });
          }
        },
        [Crepe.Feature.TopBar]: {
          buildTopBar: (builder) => {
            const existingItems = new Map(
              builder.build().flatMap((group) => group.items.map((item) => [item.key, item] as const))
            );
            const addExisting = (group: ReturnType<typeof builder.addGroup>, key: string) => {
              const item = existingItems.get(key);
              if (item) group.addItem(key, item);
            };
            const addToggleExisting = (
              group: ReturnType<typeof builder.addGroup>,
              key: string,
              active: (ctx: Ctx) => boolean,
              onRun: (ctx: Ctx) => void
            ) => {
              const item = existingItems.get(key);
              if (item) group.addItem(key, { ...item, active, onRun });
            };

            builder.clear();

            const insertGroup = builder.addGroup("document-insert", "插入");
            addExisting(insertGroup, "code-block");
            insertGroup.addItem(
              "table",
              existingItems.get("table") ?? {
                icon: insertTableIcon,
                active: () => false,
                onRun: (ctx: Ctx) => {
                  insertTable(ctx);
                }
              }
            );
            const imageItem = existingItems.get("image");
            if (imageItem) {
              insertGroup.addItem("image", {
                ...imageItem,
                active: () => false,
                onRun: (ctx: Ctx) => {
                  chooseAndInsertImage(ctx, (message) => onErrorRef.current(message));
                }
              });
            }

            const listGroup = builder.addGroup("document-list", "列表");
            addToggleExisting(
              listGroup,
              "ordered-list",
              (ctx) => isListActive(ctx, "ordered"),
              (ctx) => {
                toggleList(ctx, "ordered");
              }
            );
            addToggleExisting(
              listGroup,
              "bullet-list",
              (ctx) => isListActive(ctx, "bullet"),
              (ctx) => {
                toggleList(ctx, "bullet");
              }
            );
            addToggleExisting(
              listGroup,
              "task-list",
              (ctx) => isListActive(ctx, "task"),
              (ctx) => {
                toggleList(ctx, "task");
              }
            );
            addToggleExisting(
              listGroup,
              "quote",
              isQuoteActive,
              (ctx) => {
                toggleQuote(ctx);
              }
            );

            const structureGroup = builder.addGroup("document-structure", "结构");
            structureGroup
              .addItem("heading-1", {
                icon: headingOneIcon,
                active: (ctx: Ctx) => isHeadingLevelActive(ctx, 1),
                onRun: (ctx: Ctx) => {
                  toggleHeadingLevel(ctx, 1);
                }
              })
              .addItem("heading-2", {
                icon: headingTwoIcon,
                active: (ctx: Ctx) => isHeadingLevelActive(ctx, 2),
                onRun: (ctx: Ctx) => {
                  toggleHeadingLevel(ctx, 2);
                }
              })
              .addItem("heading-3", {
                icon: headingThreeIcon,
                active: (ctx: Ctx) => isHeadingLevelActive(ctx, 3),
                onRun: (ctx: Ctx) => {
                  toggleHeadingLevel(ctx, 3);
                }
              });
            addExisting(structureGroup, "hr");

            const referenceGroup = builder.addGroup("document-reference", "引用");
            addToggleExisting(referenceGroup, "link", isLinkActive, (ctx) => {
              toggleInlineLink(ctx);
            });
            const formulaItem = existingItems.get("math");
            referenceGroup.addItem("math", {
              ...(formulaItem ?? { icon: formulaBlockIcon }),
              active: isFormulaBlockActive,
              onRun: (ctx: Ctx) => {
                toggleFormulaBlock(ctx);
              }
            });
            referenceGroup.addItem("inline-math", {
              icon: inlineMathIcon,
              active: (ctx: Ctx) => {
                const selection = ctx.get(editorViewCtx).state.selection;
                return selection instanceof NodeSelection && selection.node.type.name === "math_inline";
              },
              onRun: (ctx: Ctx) => {
                insertInlineMath(ctx);
              }
            });

            const formattingGroup = builder.addGroup("document-formatting", "格式");
            addExisting(formattingGroup, "bold");
            addExisting(formattingGroup, "italic");
            formattingGroup.addItem("underline", {
              icon: underlineIcon,
              active: (ctx) => isMarkActive(ctx, "markdownUnderline"),
              onRun: (ctx) => {
                toggleSchemaMark(ctx, "markdownUnderline");
              }
            });
            addExisting(formattingGroup, "code");
            addExisting(formattingGroup, "strikethrough");
            formattingGroup.addItem("highlight", {
              icon: highlightIcon,
              active: (ctx) => isMarkActive(ctx, "markdownHighlight"),
              onRun: (ctx) => {
                toggleSchemaMark(ctx, "markdownHighlight");
              }
            });
          }
        }
      }
    });
    crepeInstance = crepe;
    crepe.editor
      .config((ctx) => {
        const options = ctx.get(remarkStringifyOptionsCtx);
        const customMarkHandler =
          (tag: string) =>
          (
            node: MarkdownAstNode,
            _parent: unknown,
            state: { containerPhrasing: (target: MarkdownAstNode, info: unknown) => string },
            info: unknown
          ) =>
            `<${tag}>${state.containerPhrasing(node, info)}</${tag}>`;
        ctx.set(remarkStringifyOptionsCtx, {
          ...options,
          handlers: {
            ...options.handlers,
            markdownUnderline: customMarkHandler("u"),
            markdownHighlight: customMarkHandler("mark")
          } as typeof options.handlers
        });
      })
      .use(markdownCustomMarksRemark)
      .use(underlineSchema)
      .use(highlightSchema)
      .use(markdownTableSchema)
      .use(markdownInlineLinkEditor)
      .use(markdownImageSourceEditor)
      .use(markdownTableToolbar)
      .use(markdownToolbarShortcuts);
    crepe.on((listener) => {
      listener.markdownUpdated((_ctx, nextMarkdown, previousMarkdown) => {
        if (ready && nextMarkdown !== previousMarkdown) onChangeRef.current(nextMarkdown);
      });
    });
      return crepe.create().then(() => {
        ready = !disposed;
        if (!disposed && rootRef.current) {
          applyToolbarTooltips(rootRef.current);
          toolbarObserver = new MutationObserver(() => {
            if (rootRef.current) applyToolbarTooltips(rootRef.current);
          });
          toolbarObserver.observe(rootRef.current, { childList: true, subtree: true });
        }
      });
    })
      .catch((error) => onErrorRef.current(`编辑器加载失败：${readError(error)}`));
    return () => {
      disposed = true;
      editorRoot.removeEventListener("load", handleImageLoad, true);
      editorRoot.removeEventListener("error", handleImageError, true);
      toolbarObserver?.disconnect();
      void creation.then(() => crepeInstance?.destroy()).catch(() => undefined);
    };
  }, [documentId]);

  return <div className="markdownCrepeRoot" ref={rootRef} />;
}

export function MarkdownWorkspace({
  onDirtyChange,
  onRegisterLeaveHandlers
}: MarkdownWorkspaceProps) {
  const [categories, setCategories] = useState<MarkdownCategory[]>([]);
  const [documents, setDocuments] = useState<MarkdownDocument[]>([]);
  const [selectedId, setSelectedId] = useState<number>();
  const [selectedFolderId, setSelectedFolderId] = useState<number | null>(null);
  const [expandedFolderIds, setExpandedFolderIds] = useState<Set<number>>(new Set());
  const [search, setSearch] = useState("");
  const [title, setTitle] = useState("");
  const [content, setContent] = useState("");
  const [categoryId, setCategoryId] = useState<number | null>(null);
  const [sourcePath, setSourcePath] = useState<string | null>(null);
  const [saveState, setSaveState] = useState<SaveState>("idle");
  const [busy, setBusy] = useState(false);
  const [treeOpen, setTreeOpen] = useState(true);
  const [infoOpen, setInfoOpen] = useState(false);
  const [actionsOpen, setActionsOpen] = useState(false);
  const [folderMenu, setFolderMenu] = useState<FolderMenu | null>(null);
  const [documentMenu, setDocumentMenu] = useState<DocumentMenu | null>(null);
  const [rootMenu, setRootMenu] = useState<RootMenu | null>(null);
  const [folderDraft, setFolderDraft] = useState<FolderDraft | null>(null);
  const [folderRenameDraft, setFolderRenameDraft] = useState<FolderRenameDraft | null>(null);
  const [draggedDocumentId, setDraggedDocumentId] = useState<number | null>(null);
  const [dropTargetCategoryId, setDropTargetCategoryId] = useState<number | null>(null);
  const [movingDocumentId, setMovingDocumentId] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const toastMessage = error ?? notice;
  const toastKind = error ? "error" : notice ? "notice" : null;
  const titleRef = useRef<HTMLInputElement>(null);
  const folderDraftRef = useRef<HTMLInputElement>(null);
  const folderRenameRef = useRef<HTMLInputElement>(null);
  const editorCanvasRef = useRef<HTMLDivElement>(null);
  const draggedDocumentIdRef = useRef<number | null>(null);
  const activeDocumentRef = useRef<ActiveMarkdownDocument>({
    id: undefined as number | undefined,
    title: "",
    content: "",
    categoryId: null as number | null,
    sourcePath: null as string | null,
    dirty: false
  });
  const saveTrackerRef = useRef(new MarkdownSaveTracker());
  const writeQueueRef = useRef(new MarkdownWriteQueue());

  const selectedDocument = documents.find((document) => document.id === selectedId);
  const normalizedSearch = search.trim().toLocaleLowerCase();
  const visibleDocuments = useMemo(
    () =>
      documents.filter((document) =>
        normalizedSearch
          ? `${document.title}\n${document.content}`.toLocaleLowerCase().includes(normalizedSearch)
          : true
      ),
    [documents, normalizedSearch]
  );
  const categoriesByParent = useMemo(() => {
    const map = new Map<number | null, MarkdownCategory[]>();
    categories.forEach((category) => {
      const siblings = map.get(category.parent_id) ?? [];
      siblings.push(category);
      map.set(category.parent_id, siblings);
    });
    map.forEach((siblings) =>
      siblings.sort((left, right) => left.sort_order - right.sort_order || left.name.localeCompare(right.name, "zh-CN"))
    );
    return map;
  }, [categories]);
  const documentsByCategory = useMemo(() => {
    const map = new Map<number | null, MarkdownDocument[]>();
    visibleDocuments.forEach((document) => {
      const siblings = map.get(document.category_id) ?? [];
      siblings.push(document);
      map.set(document.category_id, siblings);
    });
    return map;
  }, [visibleDocuments]);
  const wordCount = content.trim() ? content.trim().split(/\s+/u).length : 0;

  const hydrateDocument = useCallback((document?: MarkdownDocument) => {
    saveTrackerRef.current.activate(document?.id);
    activeDocumentRef.current = {
      id: document?.id,
      title: document?.title ?? "",
      content: document?.content ?? "",
      categoryId: document?.category_id ?? null,
      sourcePath: document?.source_path ?? null,
      dirty: false
    };
    setSelectedId(document?.id);
    setTitle(document?.title ?? "");
    setContent(document?.content ?? "");
    setCategoryId(document?.category_id ?? null);
    setSourcePath(document?.source_path ?? null);
    setSaveState(document ? "saved" : "idle");
  }, []);

  const markDocumentDirty = useCallback(
    (patch: Partial<Omit<ActiveMarkdownDocument, "id" | "dirty">>) => {
      activeDocumentRef.current = {
        ...activeDocumentRef.current,
        ...patch,
        dirty: true
      };
      saveTrackerRef.current.markChanged();
      setSaveState("dirty");
    },
    []
  );

  const loadLibrary = useCallback(
    async (preferredId?: number) => {
      setBusy(true);
      setError(null);
      try {
        const [nextCategories, nextDocuments] = await Promise.all([
          api.listMarkdownCategories(),
          api.listMarkdownDocuments()
        ]);
        setCategories(nextCategories);
        setDocuments(nextDocuments);
        setExpandedFolderIds((current) => {
          const availableIds = new Set(nextCategories.map((category) => category.id));
          return new Set([...current].filter((categoryId) => availableIds.has(categoryId)));
        });
        const nextId = preferredId ?? selectedId ?? nextDocuments[0]?.id;
        hydrateDocument(nextDocuments.find((document) => document.id === nextId) ?? nextDocuments[0]);
        return true;
      } catch (err) {
        setError(readError(err));
        return false;
      } finally {
        setBusy(false);
      }
    },
    [hydrateDocument, selectedId]
  );

  useEffect(() => {
    void loadLibrary();
  }, []);

  const persistCurrentDocument = useCallback(async () => {
    const snapshot = { ...activeDocumentRef.current };
    const documentId = snapshot.id;
    if (!documentId) return true;
    const token = saveTrackerRef.current.capture();
    if (!token || token.documentId !== documentId) return false;
    const nextTitle = snapshot.title.trim();
    if (!nextTitle) {
      if (saveTrackerRef.current.isCurrent(token)) {
        setError("笔记标题不能为空");
        setSaveState("error");
      }
      return false;
    }
    if (saveTrackerRef.current.isCurrent(token)) setSaveState("saving");
    try {
      const updated = await writeQueueRef.current.enqueue(() =>
        api.updateMarkdownDocument({
          id: documentId,
          title: nextTitle,
          content: snapshot.content,
          category_id: snapshot.categoryId,
          source_path: snapshot.sourcePath
        })
      );
      setDocuments((current) => current.map((document) => (document.id === updated.id ? updated : document)));
      if (saveTrackerRef.current.isCurrent(token)) {
        activeDocumentRef.current = {
          id: updated.id,
          title: updated.title,
          content: updated.content,
          categoryId: updated.category_id,
          sourcePath: updated.source_path,
          dirty: false
        };
        setTitle(updated.title);
        setContent(updated.content);
        setCategoryId(updated.category_id);
        setSourcePath(updated.source_path);
        setSaveState("saved");
        setError(null);
      }
      return true;
    } catch (err) {
      if (saveTrackerRef.current.isCurrent(token)) {
        activeDocumentRef.current = { ...activeDocumentRef.current, dirty: true };
        setSaveState("error");
        setError(`自动保存失败：${readError(err)}`);
      }
      return false;
    }
  }, []);

  const saveBeforeDocumentChange = useCallback(async () => {
    for (let attempt = 0; attempt < 5; attempt += 1) {
      if (!activeDocumentRef.current.dirty) return true;
      if (!(await persistCurrentDocument())) return false;
    }
    if (activeDocumentRef.current.dirty) {
      setError("笔记仍在发生变化，请稍后重试");
      return false;
    }
    return true;
  }, [persistCurrentDocument]);

  useEffect(() => {
    if (saveState !== "dirty" || !selectedId) return;
    const timer = window.setTimeout(() => void persistCurrentDocument(), 700);
    return () => window.clearTimeout(timer);
  }, [persistCurrentDocument, saveState, selectedId]);

  useEffect(() => {
    const dirty = saveState === "dirty" || saveState === "saving" || saveState === "error";
    onDirtyChange?.(dirty);
  }, [onDirtyChange, saveState]);

  useEffect(() => {
    if (!toastKind || !toastMessage) return;
    const timer = window.setTimeout(() => {
      if (toastKind === "error") {
        setError((current) => (current === toastMessage ? null : current));
      } else {
        setNotice((current) => (current === toastMessage ? null : current));
      }
    }, 4500);
    return () => window.clearTimeout(timer);
  }, [toastKind, toastMessage]);

  useEffect(() => {
    onRegisterLeaveHandlers?.({
      save: saveBeforeDocumentChange,
      discard: () => onDirtyChange?.(false)
    });
    return () => {
      onRegisterLeaveHandlers?.(null);
    };
  }, [onDirtyChange, onRegisterLeaveHandlers, saveBeforeDocumentChange]);

  useEffect(
    () => () => {
      onDirtyChange?.(false);
    },
    [onDirtyChange]
  );

  useEffect(() => {
    const closeMenus = () => {
      setFolderMenu(null);
      setDocumentMenu(null);
      setRootMenu(null);
      setActionsOpen(false);
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") closeMenus();
    };
    window.addEventListener("pointerdown", closeMenus);
    window.addEventListener("keydown", closeOnEscape);
    return () => {
      window.removeEventListener("pointerdown", closeMenus);
      window.removeEventListener("keydown", closeOnEscape);
    };
  }, []);

  useEffect(() => {
    if (!folderDraft) return;
    window.setTimeout(() => folderDraftRef.current?.focus(), 0);
  }, [folderDraft?.parentId]);

  useEffect(() => {
    if (!folderRenameDraft) return;
    window.setTimeout(() => {
      folderRenameRef.current?.focus();
      folderRenameRef.current?.select();
    }, 0);
  }, [folderRenameDraft?.categoryId]);

  const createDocument = useCallback(
    async (targetCategoryId: number | null = selectedFolderId) => {
      if (!(await saveBeforeDocumentChange())) return;
      setBusy(true);
      setError(null);
      try {
        const created = await api.createMarkdownDocument({
          title: defaultDocumentTitle(),
          content: "",
          category_id: targetCategoryId
        });
        setDocuments((current) => [created, ...current]);
        if (targetCategoryId) {
          setExpandedFolderIds((current) => new Set(current).add(targetCategoryId));
        }
        hydrateDocument(created);
        window.setTimeout(() => titleRef.current?.select(), 0);
      } catch (err) {
        setError(readError(err));
      } finally {
        setBusy(false);
      }
    },
    [hydrateDocument, saveBeforeDocumentChange, selectedFolderId]
  );

  function beginCreateFolder(parentId: number | null = null) {
    setFolderMenu(null);
    setRootMenu(null);
    setFolderRenameDraft(null);
    setSelectedFolderId(parentId);
    setFolderDraft({ parentId, name: "" });
    if (parentId !== null) {
      setExpandedFolderIds((current) => new Set(current).add(parentId));
    }
  }

  async function commitFolderDraft() {
    const draft = folderDraft;
    const name = draft?.name.trim();
    if (!draft || !name) {
      setFolderDraft(null);
      return;
    }
    setFolderDraft(null);
    try {
      const created = await api.createMarkdownCategory(name, draft.parentId);
      setCategories((current) => [...current, created]);
      setExpandedFolderIds((current) => {
        const next = new Set(current);
        next.add(created.id);
        if (draft.parentId !== null) next.add(draft.parentId);
        return next;
      });
      setSelectedFolderId(created.id);
    } catch (err) {
      setError(readError(err));
    }
  }

  function beginRenameFolder(category: MarkdownCategory) {
    setFolderMenu(null);
    setFolderDraft(null);
    setSelectedFolderId(category.id);
    setFolderRenameDraft({
      categoryId: category.id,
      name: category.name
    });
  }

  async function commitFolderRename(category: MarkdownCategory) {
    const draft = folderRenameDraft;
    if (!draft || draft.categoryId !== category.id) return;
    const name = draft.name.trim();
    setFolderRenameDraft(null);
    if (!name || name === category.name) return;
    try {
      const updated = await api.updateMarkdownCategory({
        id: category.id,
        name,
        parent_id: category.parent_id,
        sort_order: category.sort_order
      });
      setCategories((current) => current.map((item) => (item.id === updated.id ? updated : item)));
      setNotice("文件夹已重命名");
    } catch (err) {
      setError(readError(err));
    }
  }

  async function deleteFolder(category: MarkdownCategory) {
    const hasChildren =
      categories.some((item) => item.parent_id === category.id) ||
      documents.some((document) => document.category_id === category.id);
    if (hasChildren) {
      setError("文件夹中有子文件或子文件夹，请先删除子文件和子文件夹后再删除父文件夹");
      return;
    }
    if (!window.confirm(`确定删除空文件夹“${category.name}”吗？`)) return;
    if (!(await saveBeforeDocumentChange())) return;
    try {
      await api.deleteMarkdownCategory(category.id);
      await loadLibrary(selectedId);
      setSelectedFolderId(null);
      setNotice("文件夹已删除");
    } catch (err) {
      setError(readError(err));
    }
  }

  async function chooseImportFilePath(linkFile: boolean) {
    if (!isTauriRuntime()) throw new Error("本地文件操作仅在桌面应用中可用");
    const selected = await open({
      multiple: false,
      directory: false,
      filters: linkFile
        ? [{ name: "Markdown", extensions: [...MARKDOWN_FILE_EXTENSIONS] }]
        : [{ name: "Markdown / TXT / JSON", extensions: [...MARKDOWN_IMPORT_EXTENSIONS] }]
    });
    return typeof selected === "string" ? selected : null;
  }

  async function importDocumentFile(targetCategoryId: number | null, linkFile = false) {
    if (!(await saveBeforeDocumentChange())) return;
    try {
      const path = await chooseImportFilePath(linkFile);
      if (!path) return;
      setBusy(true);
      const file = await api.readMarkdownFile(path);
      const created = await api.createMarkdownDocument({
        title: fileTitle(file.file_name),
        content: file.content,
        category_id: targetCategoryId,
        source_path: linkFile ? file.path : null
      });
      setDocuments((current) => [created, ...current]);
      hydrateDocument(created);
      setNotice(linkFile ? "Markdown 文件已打开并关联" : "文件已导入笔记库");
    } catch (err) {
      setError(readError(err));
    } finally {
      setBusy(false);
    }
  }

  const saveLinkedFile = useCallback(async (forceChoosePath = false) => {
    if (!activeDocumentRef.current.id) return;
    if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
    setBusy(true);
    try {
      if (!(await saveBeforeDocumentChange())) return;
      let path = forceChoosePath ? null : activeDocumentRef.current.sourcePath;
      if (!path) {
        if (!isTauriRuntime()) throw new Error("本地文件操作仅在桌面应用中可用");
        path = await save({
          defaultPath: `${safeFileName(activeDocumentRef.current.title)}.md`,
          filters: [{ name: "Markdown", extensions: ["md"] }]
        });
      }
      if (!path) return;
      await api.writeMarkdownFile(path, activeDocumentRef.current.content);
      if (path !== activeDocumentRef.current.sourcePath) {
        activeDocumentRef.current = {
          ...activeDocumentRef.current,
          sourcePath: path,
          dirty: true
        };
        saveTrackerRef.current.markChanged();
        setSourcePath(path);
        setSaveState("dirty");
        if (!(await saveBeforeDocumentChange())) return;
      }
      setNotice("Markdown 文件已保存");
    } catch (err) {
      setError(readError(err));
    } finally {
      setBusy(false);
    }
  }, [saveBeforeDocumentChange]);

  useEffect(() => {
    const handleShortcut = (event: KeyboardEvent) => {
      if (!(event.ctrlKey || event.metaKey)) return;
      if (event.key.toLocaleLowerCase() === "n") {
        event.preventDefault();
        void createDocument();
      }
      if (event.key.toLocaleLowerCase() === "s") {
        event.preventDefault();
        if (activeDocumentRef.current.sourcePath) void saveLinkedFile(false);
        else void persistCurrentDocument();
      }
    };
    window.addEventListener("keydown", handleShortcut);
    return () => window.removeEventListener("keydown", handleShortcut);
  }, [createDocument, persistCurrentDocument, saveLinkedFile]);

  async function exportFolder(category: MarkdownCategory) {
    if (!(await saveBeforeDocumentChange())) return;
    try {
      if (!isTauriRuntime()) throw new Error("文件夹导出仅在桌面应用中可用");
      const directory = await open({ directory: true, multiple: false });
      if (typeof directory !== "string") return;
      const result = await api.exportMarkdownFolder(category.id, directory);
      setNotice(`已导出 ${result.item_count} 篇笔记到 ${result.path}`);
    } catch (err) {
      setError(readError(err));
    }
  }

  async function deleteDocument() {
    if (!selectedId || !window.confirm(`确定删除笔记“${title}”吗？`)) return;
    try {
      await api.deleteMarkdownDocument(selectedId);
      const remaining = documents.filter((document) => document.id !== selectedId);
      setDocuments(remaining);
      hydrateDocument(remaining[0]);
      setNotice("笔记已删除");
    } catch (err) {
      setError(readError(err));
    }
  }

  async function copyDocument(document: MarkdownDocument) {
    if (!(await saveBeforeDocumentChange())) return;
    try {
      const active = activeDocumentRef.current;
      const source =
        active.id === document.id
          ? {
              ...document,
              title: active.title,
              content: active.content,
              category_id: active.categoryId,
              source_path: active.sourcePath
            }
          : document;
      const copied = await api.createMarkdownDocument({
        title: `${source.title} - 副本`,
        content: source.content,
        category_id: source.category_id,
        source_path: null
      });
      setDocuments((current) => [copied, ...current]);
      hydrateDocument(copied);
      setSelectedFolderId(copied.category_id);
      setNotice("已复制笔记");
    } catch (err) {
      setError(readError(err));
    }
  }

  async function deleteMenuDocument(document: MarkdownDocument) {
    if (!window.confirm(`确定删除笔记“${document.title}”吗？`)) return;
    try {
      await api.deleteMarkdownDocument(document.id);
      const remaining = documents.filter((item) => item.id !== document.id);
      setDocuments(remaining);
      if (selectedId === document.id) hydrateDocument(remaining[0]);
      setNotice("笔记已删除");
    } catch (err) {
      setError(readError(err));
    }
  }

  async function chooseDocumentExportPath(document: MarkdownDocument, extension: string, label: string) {
    if (!isTauriRuntime()) throw new Error("文件导出仅在桌面应用中可用");
    return save({
      defaultPath: `${safeFileName(document.title)}.${extension}`,
      filters: [{ name: label, extensions: [extension] }]
    });
  }

  function renderedDocumentElement() {
    return editorCanvasRef.current?.querySelector<HTMLElement>(".ProseMirror") ?? null;
  }

  async function exportDocument(document: MarkdownDocument, format: "markdown" | "html" | "pdf" | "image") {
    setBusy(true);
    setError(null);
    try {
      if (format === "markdown") {
        const path = await chooseDocumentExportPath(document, "md", "Markdown");
        if (!path) return;
        await api.writeMarkdownFile(path, document.content);
        setNotice(`已导出 Markdown：${path}`);
        return;
      }

      const rendered = renderedDocumentElement();
      if (format === "html") {
        const path = await chooseDocumentExportPath(document, "html", "HTML");
        if (!path) return;
        const body = rendered
          ? cleanMarkdownEditorHtml(rendered)
          : markdownTextFallbackHtml(document.content);
        const bytes = Array.from(
          new TextEncoder().encode(standaloneMarkdownHtml(document.title, body))
        );
        await api.writeMarkdownExportFile(path, bytes);
        setNotice(`已导出 HTML：${path}`);
        return;
      }

      if (!rendered) throw new Error("笔记内容尚未渲染，请稍后再试");
      const { default: html2canvas } = await import("html2canvas");
      const snapshot = createMarkdownExportSnapshot(rendered);
      let canvas: HTMLCanvasElement;
      try {
        canvas = await html2canvas(snapshot.content, {
          backgroundColor: "#ffffff",
          scale: 1.5,
          useCORS: true,
          logging: false,
          width: snapshot.content.scrollWidth,
          height: snapshot.content.scrollHeight,
          windowWidth: snapshot.content.scrollWidth,
          windowHeight: snapshot.content.scrollHeight
        });
      } finally {
        snapshot.root.remove();
      }
      if (format === "image") {
        const path = await chooseDocumentExportPath(document, "png", "PNG 图片");
        if (!path) return;
        const blob = await new Promise<Blob>((resolve, reject) =>
          canvas.toBlob((value) => (value ? resolve(value) : reject(new Error("生成图片失败"))), "image/png")
        );
        await api.writeMarkdownExportFile(path, Array.from(new Uint8Array(await blob.arrayBuffer())));
        setNotice(`已导出图片：${path}`);
        return;
      }

      const path = await chooseDocumentExportPath(document, "pdf", "PDF");
      if (!path) return;
      const { jsPDF } = await import("jspdf");
      const pdf = new jsPDF({
        orientation: "portrait",
        unit: "mm",
        format: "a4",
        compress: true
      });
      const layout = calculateMarkdownPdfLayout(canvas.width, canvas.height);
      layout.slices.forEach((slice, index) => {
        if (index > 0) pdf.addPage();

        const pageCanvas = window.document.createElement("canvas");
        pageCanvas.width = canvas.width;
        pageCanvas.height = slice.sourceHeight;
        const pageContext = pageCanvas.getContext("2d");
        if (!pageContext) throw new Error("无法创建 PDF 页面画布");
        pageContext.fillStyle = "#ffffff";
        pageContext.fillRect(0, 0, pageCanvas.width, pageCanvas.height);
        pageContext.drawImage(
          canvas,
          0,
          slice.sourceY,
          canvas.width,
          slice.sourceHeight,
          0,
          0,
          pageCanvas.width,
          pageCanvas.height
        );
        pdf.addImage(
          pageCanvas.toDataURL("image/png"),
          "PNG",
          layout.marginMm,
          layout.marginMm,
          layout.imageWidthMm,
          slice.imageHeightMm,
          undefined,
          "FAST"
        );
      });
      await api.writeMarkdownExportFile(path, Array.from(new Uint8Array(pdf.output("arraybuffer"))));
      setNotice(`已导出 PDF：${path}`);
    } catch (err) {
      setError(readError(err));
    } finally {
      setBusy(false);
    }
  }

  async function selectDocument(document: MarkdownDocument) {
    if (document.id === activeDocumentRef.current.id) {
      setSelectedFolderId(document.category_id);
      return true;
    }
    if (!(await saveBeforeDocumentChange())) return false;
    hydrateDocument(document);
    setSelectedFolderId(document.category_id);
    setInfoOpen(false);
    return true;
  }

  function toggleFolder(categoryIdToToggle: number) {
    setExpandedFolderIds((current) => {
      const next = new Set(current);
      if (next.has(categoryIdToToggle)) next.delete(categoryIdToToggle);
      else next.add(categoryIdToToggle);
      return next;
    });
  }

  function showFolderMenu(event: ReactMouseEvent, category: MarkdownCategory) {
    event.preventDefault();
    event.stopPropagation();
    setDocumentMenu(null);
    setRootMenu(null);
    setFolderMenu({
      category,
      x: Math.min(event.clientX, window.innerWidth - 240),
      y: Math.min(event.clientY, window.innerHeight - 330)
    });
  }

  function showDocumentMenu(event: ReactMouseEvent, document: MarkdownDocument) {
    event.preventDefault();
    event.stopPropagation();
    setFolderMenu(null);
    setRootMenu(null);
    const x = Math.min(event.clientX, window.innerWidth - 310);
    const y = Math.min(event.clientY, window.innerHeight - 390);
    void (async () => {
      if (!(await selectDocument(document))) return;
      const active = activeDocumentRef.current;
      const snapshot =
        active.id === document.id
          ? {
              ...document,
              title: active.title,
              content: active.content,
              category_id: active.categoryId,
              source_path: active.sourcePath
            }
          : document;
      setDocumentMenu({ document: snapshot, x, y });
    })();
  }

  function showRootMenu(event: ReactMouseEvent<HTMLDivElement>) {
    if (
      event.target instanceof Element &&
      event.target.closest(".markdownTreeFolder, .markdownTreeNote, .markdownFolderDraft")
    ) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    setFolderMenu(null);
    setDocumentMenu(null);
    setActionsOpen(false);
    setRootMenu({
      x: Math.max(8, Math.min(event.clientX, window.innerWidth - 240)),
      y: Math.max(8, Math.min(event.clientY, window.innerHeight - 210))
    });
  }

  async function refreshLibrary() {
    setRootMenu(null);
    if (!(await saveBeforeDocumentChange())) return;
    if (await loadLibrary(selectedId)) setNotice("笔记库已刷新");
  }

  async function moveDocumentToFolder(documentId: number, targetCategoryId: number) {
    const document = documents.find((item) => item.id === documentId);
    if (!document || document.category_id === targetCategoryId || movingDocumentId !== null) return;

    setMovingDocumentId(documentId);
    setError(null);
    try {
      if (
        activeDocumentRef.current.id === documentId &&
        activeDocumentRef.current.dirty &&
        !(await saveBeforeDocumentChange())
      ) {
        return;
      }

      const active = activeDocumentRef.current;
      const source =
        active.id === documentId
          ? {
              title: active.title,
              content: active.content,
              source_path: active.sourcePath
            }
          : {
              title: document.title,
              content: document.content,
              source_path: document.source_path
            };
      if (active.id === documentId) {
        activeDocumentRef.current = {
          ...active,
          categoryId: targetCategoryId
        };
      }
      const updated = await writeQueueRef.current.enqueue(() =>
        api.updateMarkdownDocument({
          id: documentId,
          title: source.title,
          content: source.content,
          category_id: targetCategoryId,
          source_path: source.source_path
        })
      );

      setDocuments((current) => current.map((item) => (item.id === updated.id ? updated : item)));
      if (activeDocumentRef.current.id === updated.id) {
        activeDocumentRef.current = {
          ...activeDocumentRef.current,
          categoryId: updated.category_id
        };
        setCategoryId(updated.category_id);
      }
      setSelectedFolderId(updated.category_id);
      setExpandedFolderIds((current) => new Set(current).add(targetCategoryId));
    } catch (err) {
      if (
        activeDocumentRef.current.id === documentId &&
        activeDocumentRef.current.categoryId === targetCategoryId
      ) {
        activeDocumentRef.current = {
          ...activeDocumentRef.current,
          categoryId: document.category_id
        };
        setCategoryId(document.category_id);
      }
      setError(`移动笔记失败：${readError(err)}`);
    } finally {
      setMovingDocumentId(null);
    }
  }

  function handleFolderDragOver(event: ReactDragEvent<HTMLButtonElement>, categoryIdToDrop: number) {
    const documentId = draggedDocumentIdRef.current;
    if (documentId === null) return;
    const document = documents.find((item) => item.id === documentId);
    if (!document || document.category_id === categoryIdToDrop) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = "move";
    setDropTargetCategoryId(categoryIdToDrop);
  }

  function handleFolderDrop(event: ReactDragEvent<HTMLButtonElement>, categoryIdToDrop: number) {
    event.preventDefault();
    event.stopPropagation();
    const documentId = draggedDocumentIdRef.current;
    draggedDocumentIdRef.current = null;
    setDropTargetCategoryId(null);
    setDraggedDocumentId(null);
    if (documentId !== null) void moveDocumentToFolder(documentId, categoryIdToDrop);
  }

  function renderDocument(document: MarkdownDocument, depth: number) {
    const className = [
      "markdownTreeNote",
      selectedId === document.id ? "active" : "",
      draggedDocumentId === document.id ? "dragging" : "",
      movingDocumentId === document.id ? "moving" : ""
    ]
      .filter(Boolean)
      .join(" ");
    return (
      <button
        type="button"
        className={className}
        style={{ "--tree-depth": depth } as CSSProperties}
        title={`${document.title}（可拖动到文件夹）`}
        draggable={movingDocumentId === null}
        aria-grabbed={draggedDocumentId === document.id}
        onDragStart={(event) => {
          draggedDocumentIdRef.current = document.id;
          setDraggedDocumentId(document.id);
          setDropTargetCategoryId(null);
          event.dataTransfer.effectAllowed = "move";
          try {
            event.dataTransfer.setData("text/plain", String(document.id));
          } catch {
            // WebView drag state is held by draggedDocumentIdRef; payload support is optional.
          }
        }}
        onDragEnd={() => {
          draggedDocumentIdRef.current = null;
          setDraggedDocumentId(null);
          setDropTargetCategoryId(null);
        }}
        onClick={() => void selectDocument(document)}
        onContextMenu={(event) => showDocumentMenu(event, document)}
        key={document.id}
      >
        <FileText size={15} />
        <span>{document.title}</span>
      </button>
    );
  }

  function renderFolderDraft(depth: number) {
    if (!folderDraft) return null;
    return (
      <div
        className="markdownFolderDraft"
        style={{ "--tree-depth": depth } as CSSProperties}
      >
        <ChevronRight size={15} />
        <Folder size={16} />
        <input
          ref={folderDraftRef}
          value={folderDraft.name}
          aria-label="新文件夹名称"
          placeholder="新建文件夹"
          onChange={(event) =>
            setFolderDraft((current) => (current ? { ...current, name: event.target.value } : current))
          }
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              void commitFolderDraft();
            }
            if (event.key === "Escape") {
              event.preventDefault();
              setFolderDraft(null);
            }
          }}
          onBlur={() => void commitFolderDraft()}
        />
      </div>
    );
  }

  function renderFolder(category: MarkdownCategory, depth: number): React.ReactNode {
    const children = categoriesByParent.get(category.id) ?? [];
    const folderDocuments = documentsByCategory.get(category.id) ?? [];
    const expanded = expandedFolderIds.has(category.id);
    const renaming = folderRenameDraft?.categoryId === category.id;
    const folderClassName = [
      "markdownTreeFolder",
      selectedFolderId === category.id ? "active" : "",
      dropTargetCategoryId === category.id ? "dropTarget" : ""
    ]
      .filter(Boolean)
      .join(" ");
    return (
      <div className="markdownTreeBranch" key={category.id}>
        {renaming ? (
          <div
            className="markdownFolderDraft markdownFolderRename"
            style={{ "--tree-depth": depth } as CSSProperties}
          >
            {expanded ? <ChevronDown size={15} /> : <ChevronRight size={15} />}
            {expanded ? <FolderOpen size={16} /> : <Folder size={16} />}
            <input
              ref={folderRenameRef}
              value={folderRenameDraft.name}
              aria-label={`重命名文件夹 ${category.name}`}
              onChange={(event) =>
                setFolderRenameDraft((current) =>
                  current?.categoryId === category.id
                    ? { ...current, name: event.target.value }
                    : current
                )
              }
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  event.stopPropagation();
                  event.currentTarget.blur();
                } else if (event.key === "Escape") {
                  event.preventDefault();
                  event.stopPropagation();
                  setFolderRenameDraft(null);
                }
              }}
              onBlur={() => void commitFolderRename(category)}
            />
          </div>
        ) : (
          <button
            type="button"
            className={folderClassName}
            style={{ "--tree-depth": depth } as CSSProperties}
            onClick={() => {
              setSelectedFolderId(category.id);
              toggleFolder(category.id);
            }}
            onDragOver={(event) => handleFolderDragOver(event, category.id)}
            onDragLeave={(event) => {
              if (event.relatedTarget instanceof Node && event.currentTarget.contains(event.relatedTarget)) return;
              setDropTargetCategoryId((current) => (current === category.id ? null : current));
            }}
            onDrop={(event) => handleFolderDrop(event, category.id)}
            onContextMenu={(event) => showFolderMenu(event, category)}
            title={`${category.name}（右键查看更多操作）`}
          >
            {expanded ? <ChevronDown size={15} /> : <ChevronRight size={15} />}
            {expanded ? <FolderOpen size={16} /> : <Folder size={16} />}
            <span>{category.name}</span>
          </button>
        )}
        {expanded && (
          <div>
            {folderDraft?.parentId === category.id && renderFolderDraft(depth + 1)}
            {children.map((child) => renderFolder(child, depth + 1))}
            {folderDocuments.map((document) => renderDocument(document, depth + 1))}
          </div>
        )}
      </div>
    );
  }

  const saveStateLabel = {
    idle: "",
    dirty: "正在等待自动保存",
    saving: "正在保存…",
    saved: "已保存到 SQLite",
    error: "保存失败"
  }[saveState];

  return (
    <section className={treeOpen ? "markdownWorkspace" : "markdownWorkspace library-collapsed"}>
      <aside className="markdownLibrary">
        <div className="markdownLibraryToolbar">
          <button
            type="button"
            title="展开或折叠全部文件夹"
            onClick={() =>
              setExpandedFolderIds((current) =>
                current.size === categories.length ? new Set() : new Set(categories.map((category) => category.id))
              )
            }
          >
            <ChevronsUpDown size={18} />
          </button>
          <button type="button" title="导入 Markdown / TXT / JSON 文件" onClick={() => void importDocumentFile(selectedFolderId, false)}>
            <Upload size={18} />
          </button>
          <button type="button" title="新建文件夹" onClick={() => beginCreateFolder(null)}>
            <FolderPlus size={18} />
          </button>
          <button type="button" title="新建笔记 Ctrl+N" onClick={() => void createDocument()}>
            <FilePlus2 size={18} />
          </button>
        </div>
        <label className="markdownTreeSearch">
          <Search size={14} />
          <input value={search} onChange={(event) => setSearch(event.target.value)} placeholder="搜索笔记" />
          {search && (
            <button type="button" title="清除搜索" onClick={() => setSearch("")}>
              <X size={13} />
            </button>
          )}
        </label>
        <div className="markdownTree" onContextMenu={showRootMenu}>
          {folderDraft?.parentId === null && renderFolderDraft(0)}
          {(categoriesByParent.get(null) ?? []).map((category) => renderFolder(category, 0))}
          {(documentsByCategory.get(null) ?? []).map((document) => renderDocument(document, 0))}
          {documents.length === 0 && categories.length === 0 && !busy && !folderDraft && (
            <div className="markdownTreeEmpty">
              <FileText size={26} />
              <span>还没有 Markdown 笔记</span>
              <button type="button" onClick={() => void createDocument(null)}>新建第一篇</button>
            </div>
          )}
          {normalizedSearch && visibleDocuments.length === 0 && (
            <div className="markdownTreeEmpty">没有匹配的笔记</div>
          )}
        </div>
      </aside>

      <article className="markdownEditorPanel">
        {busy && (
          <div className="markdownBusy">
            <Loader2 className="spin" size={20} />
            正在处理…
          </div>
        )}
        {selectedDocument && selectedId ? (
          <>
            <header className="markdownDocumentHeader">
              <button
                type="button"
                className="markdownHeaderIcon"
                title={treeOpen ? "收起笔记列表" : "展开笔记列表"}
                onClick={() => setTreeOpen((current) => !current)}
              >
                {treeOpen ? <PanelLeftClose size={18} /> : <PanelLeftOpen size={18} />}
              </button>
              <input
                ref={titleRef}
                className="markdownDocumentTitleInput"
                value={title}
                aria-label="笔记标题"
                onChange={(event) => {
                  const nextTitle = event.target.value;
                  setTitle(nextTitle);
                  markDocumentDirty({ title: nextTitle });
                }}
                onBlur={() => void persistCurrentDocument()}
              />
              <div className="markdownDocumentActions">
                <span className={`markdownSaveState state-${saveState}`}>{saveStateLabel}</span>
                <button
                  type="button"
                  className={infoOpen ? "markdownHeaderIcon active" : "markdownHeaderIcon"}
                  title="笔记信息"
                  onClick={() => setInfoOpen((current) => !current)}
                >
                  <Info size={18} />
                </button>
                <button
                  type="button"
                  className="markdownHeaderIcon"
                  title={sourcePath ? "保存到关联文件 Ctrl+S" : "另存为 Markdown 文件"}
                  onClick={() => void saveLinkedFile(false)}
                >
                  <Save size={18} />
                </button>
                <div className="markdownActionsAnchor" onPointerDown={(event) => event.stopPropagation()}>
                  <button
                    type="button"
                    className={actionsOpen ? "markdownHeaderIcon active" : "markdownHeaderIcon"}
                    title="更多操作"
                    onClick={() => setActionsOpen((current) => !current)}
                  >
                    <MoreHorizontal size={20} />
                  </button>
                  {actionsOpen && (
                    <div className="markdownPopupMenu markdownDocumentMenu">
                      <button type="button" onClick={() => { setActionsOpen(false); void importDocumentFile(selectedFolderId, true); }}>
                        <FolderOpen size={17} />打开并关联 Markdown
                      </button>
                      <button type="button" onClick={() => { setActionsOpen(false); void importDocumentFile(selectedFolderId, false); }}>
                        <Upload size={17} />导入文件副本
                      </button>
                      <div className="markdownMenuDivider" />
                      <button type="button" onClick={() => { setActionsOpen(false); void saveLinkedFile(false); }}>
                        <Save size={17} />保存关联文件
                      </button>
                      <button type="button" onClick={() => { setActionsOpen(false); void saveLinkedFile(true); }}>
                        <FileDown size={17} />另存为 Markdown
                      </button>
                      <div className="markdownMenuDivider" />
                      <button type="button" className="danger" onClick={() => { setActionsOpen(false); void deleteDocument(); }}>
                        <Trash2 size={17} />删除笔记
                      </button>
                    </div>
                  )}
                </div>
              </div>
            </header>
            <div className="markdownEditorCanvas" ref={editorCanvasRef}>
              <CrepeEditor
                key={selectedId}
                documentId={selectedId}
                markdown={content}
                onChange={(nextContent) => {
                  setContent(nextContent);
                  markDocumentDirty({ content: nextContent });
                }}
                onError={setError}
              />
            </div>
            {infoOpen && (
              <aside className="markdownInfoDrawer">
                <div className="markdownInfoHeader">
                  <strong>笔记信息</strong>
                  <button type="button" onClick={() => setInfoOpen(false)}><X size={17} /></button>
                </div>
                <dl>
                  <div><dt>文件夹</dt><dd>{selectedDocument.category_name ?? "未分类"}</dd></div>
                  <div><dt>字数</dt><dd>{wordCount}</dd></div>
                  <div><dt>字符</dt><dd>{content.length}</dd></div>
                  <div><dt>创建时间</dt><dd>{formatDate(selectedDocument.created_at)}</dd></div>
                  <div><dt>更新时间</dt><dd>{formatDate(selectedDocument.updated_at)}</dd></div>
                  <div><dt>关联文件</dt><dd title={sourcePath ?? ""}>{sourcePath ?? "未关联"}</dd></div>
                </dl>
                <label>
                  所在文件夹
                  <select
                    value={categoryId ?? ""}
                    onChange={(event) => {
                      const nextId = event.target.value ? Number(event.target.value) : null;
                      setCategoryId(nextId);
                      setSelectedFolderId(nextId);
                      markDocumentDirty({ categoryId: nextId });
                    }}
                  >
                    <option value="">未分类</option>
                    {categories.map((category) => <option value={category.id} key={category.id}>{category.name}</option>)}
                  </select>
                </label>
              </aside>
            )}
          </>
        ) : (
          <div className="markdownWelcome">
            <CircleHelp size={38} />
            <h2>Markdown 笔记</h2>
            <p>新建一篇笔记，内容会自动保存到本地 SQLite。</p>
            <div>
              <button type="button" className="button primary" onClick={() => void createDocument(null)}>
                <FilePlus2 size={17} />新建笔记
              </button>
              <button type="button" className="button" onClick={() => void importDocumentFile(null, false)}>
                <Upload size={17} />导入文件
              </button>
            </div>
          </div>
        )}
        {toastMessage && (
          <div
            className={`markdownToast ${toastKind === "error" ? "error" : "success"}`}
            title={toastMessage}
            role={toastKind === "error" ? "alert" : "status"}
            aria-live={toastKind === "error" ? "assertive" : "polite"}
          >
            {toastMessage}
            <button type="button" onClick={() => { setError(null); setNotice(null); }}><X size={14} /></button>
          </div>
        )}
      </article>

      {rootMenu && (
        <div
          className="markdownPopupMenu markdownRootMenu"
          style={{ left: rootMenu.x, top: rootMenu.y }}
          aria-label="根目录操作"
          onPointerDown={(event) => event.stopPropagation()}
        >
          <button type="button" onClick={() => { setRootMenu(null); void createDocument(null); }}>
            <FilePlus2 size={17} />新建笔记
          </button>
          <button type="button" onClick={() => beginCreateFolder(null)}>
            <FolderPlus size={17} />新建文件夹
          </button>
          <button type="button" onClick={() => { setRootMenu(null); void importDocumentFile(null, false); }}>
            <Upload size={17} />导入 Markdown / TXT / JSON 文件
          </button>
          <div className="markdownMenuDivider" />
          <button type="button" onClick={() => void refreshLibrary()}>
            <RefreshCw size={17} />刷新
          </button>
        </div>
      )}

      {folderMenu && (
        <div
          className="markdownPopupMenu markdownFolderMenu"
          style={{ left: folderMenu.x, top: folderMenu.y }}
          onPointerDown={(event) => event.stopPropagation()}
        >
          <button type="button" onClick={() => { setFolderMenu(null); void createDocument(folderMenu.category.id); }}>
            <FilePlus2 size={17} />新建笔记
          </button>
          <button type="button" onClick={() => beginCreateFolder(folderMenu.category.id)}>
            <FolderPlus size={17} />新建子文件夹
          </button>
          <div className="markdownMenuDivider" />
          <button type="button" onClick={() => { setFolderMenu(null); void importDocumentFile(folderMenu.category.id, false); }}>
            <Upload size={17} />导入 Markdown / TXT / JSON 文件
          </button>
          <button type="button" onClick={() => { setFolderMenu(null); void exportFolder(folderMenu.category); }}>
            <FolderDown size={17} />导出文件夹
          </button>
          <div className="markdownMenuDivider" />
          <button type="button" onClick={() => beginRenameFolder(folderMenu.category)}>
            <Pencil size={17} />重命名
          </button>
          <button type="button" className="danger" onClick={() => { setFolderMenu(null); void deleteFolder(folderMenu.category); }}>
            <Trash2 size={17} />删除文件夹
          </button>
        </div>
      )}

      {documentMenu && (
        <div
          className="markdownPopupMenu markdownDocumentContextMenu"
          style={{ left: documentMenu.x, top: documentMenu.y }}
          onPointerDown={(event) => event.stopPropagation()}
        >
          <button type="button" onClick={() => { const target = documentMenu.document; setDocumentMenu(null); void copyDocument(target); }}>
            <Copy size={19} />复制一份
          </button>
          <button type="button" className="danger" onClick={() => { const target = documentMenu.document; setDocumentMenu(null); void deleteMenuDocument(target); }}>
            <Trash2 size={19} />删除笔记
          </button>
          <div className="markdownMenuDivider" />
          <button type="button" onClick={() => { const target = documentMenu.document; setDocumentMenu(null); void exportDocument(target, "markdown"); }}>
            <FileText size={19} />导出为 Markdown 文件
          </button>
          <button type="button" onClick={() => { const target = documentMenu.document; setDocumentMenu(null); void exportDocument(target, "html"); }}>
            <FileCode2 size={19} />导出为 HTML 文件
          </button>
          <button type="button" onClick={() => { const target = documentMenu.document; setDocumentMenu(null); void exportDocument(target, "pdf"); }}>
            <FileDown size={19} />导出为 PDF 文件
          </button>
          <button type="button" onClick={() => { const target = documentMenu.document; setDocumentMenu(null); void exportDocument(target, "image"); }}>
            <FileImage size={19} />导出为图像文件
          </button>
        </div>
      )}
    </section>
  );
}
