import type { Node as ProseMirrorNode, NodeType } from "@milkdown/kit/prose/model";
import {
  Selection,
  type Command,
  type EditorState,
  type Transaction
} from "@milkdown/kit/prose/state";
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

function cellPositionInRow(
  rowStart: number,
  row: ProseMirrorNode,
  column: number
) {
  let position = rowStart + 1;
  for (let index = 0; index < column; index += 1) {
    position += row.child(index).nodeSize;
  }
  return position;
}

function selectCell(
  transaction: Transaction,
  rowStart: number,
  row: ProseMirrorNode,
  column: number
) {
  const safeColumn = Math.min(Math.max(column, 0), row.childCount - 1);
  const cellPos = cellPositionInRow(rowStart, row, safeColumn);
  transaction.setSelection(
    Selection.near(transaction.doc.resolve(cellPos + 1), 1)
  );
}

function convertCell(
  targetType: NodeType,
  source: ProseMirrorNode,
  alignment: unknown
) {
  return targetType.createChecked(
    {
      ...source.attrs,
      alignment: normalizeAlignment(alignment)
    },
    source.content,
    source.marks
  );
}

function createEmptyCell(template: ProseMirrorNode, alignment: unknown) {
  return template.type.createAndFill({
    ...template.attrs,
    alignment: normalizeAlignment(alignment)
  });
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
      // Inserting above the header is handled by promoting a new empty header.
      canInsertRowBefore: true,
      // Deleting the header promotes the following body row. Body-row deletion
      // may leave a valid header-only GFM table.
      canDeleteRow:
        rect.top === 0
          ? rect.bottom === 1
          : rect.map.height - selectedRowCount >= 1,
      // Keep at least one column so the result remains a useful GFM table.
      canDeleteColumn: rect.map.width - selectedColumnCount >= 1
    };
  } catch {
    return null;
  }
}

export const insertMarkdownTableHeaderRowBefore: Command = (
  state,
  dispatch
) => {
  const toolbarState = getMarkdownTableToolbarState(state);
  if (!toolbarState || toolbarState.rowFrom !== 0) return false;

  let rect: ReturnType<typeof selectedRect>;
  try {
    rect = selectedRect(state);
  } catch {
    return false;
  }

  const headerRow = rect.table.firstChild;
  const bodyTemplate = rect.table.maybeChild(1);
  const bodyRowType = bodyTemplate?.type ?? state.schema.nodes.table_row;
  const bodyCellType =
    bodyTemplate?.firstChild?.type ?? state.schema.nodes.table_cell;
  if (
    !headerRow ||
    !bodyRowType ||
    !bodyCellType ||
    headerRow.childCount === 0 ||
    (bodyTemplate && headerRow.childCount !== bodyTemplate.childCount)
  ) {
    return false;
  }

  const newHeaderCells: ProseMirrorNode[] = [];
  const demotedCells: ProseMirrorNode[] = [];
  for (let column = 0; column < headerRow.childCount; column += 1) {
    const headerCell = headerRow.child(column);
    const alignment = headerCell.attrs.alignment;
    const emptyHeaderCell = createEmptyCell(headerCell, alignment);
    if (!emptyHeaderCell) return false;
    newHeaderCells.push(emptyHeaderCell);
    demotedCells.push(convertCell(bodyCellType, headerCell, alignment));
  }

  const newHeaderRow = headerRow.type.createChecked(
    headerRow.attrs,
    newHeaderCells,
    headerRow.marks
  );
  const demotedHeaderRow = bodyRowType.createChecked(
    bodyTemplate?.attrs,
    demotedCells,
    bodyTemplate?.marks
  );

  if (dispatch) {
    const transaction = state.tr.replaceWith(
      rect.tableStart,
      rect.tableStart + headerRow.nodeSize,
      [newHeaderRow, demotedHeaderRow]
    );
    const demotedRowStart = rect.tableStart + newHeaderRow.nodeSize;
    selectCell(
      transaction,
      demotedRowStart,
      demotedHeaderRow,
      toolbarState.columnFrom
    );
    dispatch(transaction.scrollIntoView());
  }
  return true;
};

const deleteMarkdownTableHeaderRow: Command = (state, dispatch) => {
  const toolbarState = getMarkdownTableToolbarState(state);
  if (
    !toolbarState ||
    toolbarState.rowFrom !== 0 ||
    toolbarState.rowTo !== 1
  ) {
    return false;
  }

  let rect: ReturnType<typeof selectedRect>;
  try {
    rect = selectedRect(state);
  } catch {
    return false;
  }

  const headerRow = rect.table.firstChild;
  const nextBodyRow = rect.table.maybeChild(1);
  if (headerRow && !nextBodyRow) {
    return deleteTable(state, dispatch);
  }
  if (
    !headerRow ||
    !nextBodyRow ||
    headerRow.childCount === 0 ||
    headerRow.childCount !== nextBodyRow.childCount
  ) {
    return false;
  }

  const promotedCells: ProseMirrorNode[] = [];
  for (let column = 0; column < headerRow.childCount; column += 1) {
    const oldHeaderCell = headerRow.child(column);
    const bodyCell = nextBodyRow.child(column);
    const alignment = oldHeaderCell.attrs.alignment;
    promotedCells.push(convertCell(oldHeaderCell.type, bodyCell, alignment));
  }

  const promotedHeaderRow = headerRow.type.createChecked(
    headerRow.attrs,
    promotedCells,
    headerRow.marks
  );

  if (dispatch) {
    const transaction = state.tr.replaceWith(
      rect.tableStart,
      rect.tableStart + headerRow.nodeSize + nextBodyRow.nodeSize,
      promotedHeaderRow
    );
    selectCell(
      transaction,
      rect.tableStart,
      promotedHeaderRow,
      toolbarState.columnFrom
    );
    dispatch(transaction.scrollIntoView());
  }
  return true;
};

export const deleteMarkdownTableRow: Command = (state, dispatch) => {
  const toolbarState = getMarkdownTableToolbarState(state);
  if (!toolbarState?.canDeleteRow) return false;
  if (toolbarState.rowFrom === 0) {
    return deleteMarkdownTableHeaderRow(state, dispatch);
  }
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
