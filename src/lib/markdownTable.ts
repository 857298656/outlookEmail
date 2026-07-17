import type { Command, EditorState } from "@milkdown/kit/prose/state";
import {
  deleteColumn,
  deleteRow,
  deleteTable,
  isInTable,
  selectedRect
} from "@milkdown/kit/prose/tables";

export type MarkdownTableAlignment = "left" | "center" | "right";

export type MarkdownTableToolbarState = {
  tablePos: number;
  rowFrom: number;
  rowTo: number;
  columnFrom: number;
  columnTo: number;
  rowCount: number;
  columnCount: number;
  alignment: MarkdownTableAlignment | null;
  canInsertRowBefore: boolean;
  canDeleteRow: boolean;
  canDeleteColumn: boolean;
};

function normalizeAlignment(value: unknown): MarkdownTableAlignment {
  return value === "center" || value === "right" ? value : "left";
}

export function getMarkdownTableToolbarState(
  state: EditorState
): MarkdownTableToolbarState | null {
  if (!isInTable(state)) return null;

  try {
    const rect = selectedRect(state);
    const selectedRowCount = rect.bottom - rect.top;
    const selectedColumnCount = rect.right - rect.left;
    const alignments = new Set<MarkdownTableAlignment>();

    for (let column = rect.left; column < rect.right; column += 1) {
      const cellPos = rect.map.map[column];
      const headerCell = cellPos === undefined ? null : rect.table.nodeAt(cellPos);
      alignments.add(normalizeAlignment(headerCell?.attrs.alignment));
    }

    return {
      tablePos: rect.tableStart - 1,
      rowFrom: rect.top,
      rowTo: rect.bottom,
      columnFrom: rect.left,
      columnTo: rect.right,
      rowCount: rect.map.height,
      columnCount: rect.map.width,
      alignment: alignments.size === 1 ? [...alignments][0]! : null,
      // GFM requires the first row to remain the table header.
      canInsertRowBefore: rect.top > 0,
      // Milkdown's GFM schema requires a header plus at least one body row.
      canDeleteRow:
        rect.top > 0 && rect.map.height - selectedRowCount >= 2,
      // Keep at least one column so the result remains a useful GFM table.
      canDeleteColumn: rect.map.width - selectedColumnCount >= 1
    };
  } catch {
    return null;
  }
}

export const deleteMarkdownTableRow: Command = (state, dispatch) => {
  const toolbarState = getMarkdownTableToolbarState(state);
  if (!toolbarState?.canDeleteRow) return false;
  return deleteRow(state, dispatch);
};

export const deleteMarkdownTableColumn: Command = (state, dispatch) => {
  const toolbarState = getMarkdownTableToolbarState(state);
  if (!toolbarState?.canDeleteColumn) return false;
  return deleteColumn(state, dispatch);
};

export const deleteMarkdownTable: Command = (state, dispatch) =>
  deleteTable(state, dispatch);

export function setMarkdownTableColumnAlignment(
  alignment: MarkdownTableAlignment
): Command {
  return (state, dispatch) => {
    if (!isInTable(state)) return false;

    let rect: ReturnType<typeof selectedRect>;
    try {
      rect = selectedRect(state);
    } catch {
      return false;
    }

    const cellPositions = new Set(
      rect.map.cellsInRect({
        left: rect.left,
        right: rect.right,
        top: 0,
        bottom: rect.map.height
      })
    );
    const transaction = state.tr;

    cellPositions.forEach((cellPos) => {
      const cell = rect.table.nodeAt(cellPos);
      if (!cell || normalizeAlignment(cell.attrs.alignment) === alignment) return;
      transaction.setNodeMarkup(rect.tableStart + cellPos, undefined, {
        ...cell.attrs,
        alignment
      });
    });

    if (!transaction.docChanged) return false;
    dispatch?.(transaction);
    return true;
  };
}
