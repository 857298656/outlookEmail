import { Schema, type Node as ProseMirrorNode } from "@milkdown/kit/prose/model";
import {
  CellSelection,
  TableMap,
  tableNodes
} from "@milkdown/kit/prose/tables";
import {
  EditorState,
  TextSelection,
  type Command
} from "@milkdown/kit/prose/state";
import { describe, expect, it } from "vitest";
import {
  deleteMarkdownTable,
  deleteMarkdownTableColumn,
  deleteMarkdownTableRow,
  getMarkdownTableToolbarState,
  setMarkdownTableColumnAlignment
} from "./markdownTable";

const tableSpecs = tableNodes({
  tableGroup: "block",
  cellContent: "paragraph",
  cellAttributes: {
    alignment: {
      default: "left"
    }
  }
});

const schema = new Schema({
  nodes: {
    doc: { content: "block+" },
    paragraph: { content: "text*", group: "block" },
    text: { group: "inline" },
    ...tableSpecs
  }
});

function createDocument(bodyRows = 2, columns = 2) {
  const headerCells = Array.from({ length: columns }, () =>
    schema.nodes.table_header!.createAndFill({ alignment: "left" })!
  );
  const headerRow = schema.nodes.table_row!.create(null, headerCells);
  const rows = Array.from({ length: bodyRows }, () => {
    const cells = Array.from({ length: columns }, () =>
      schema.nodes.table_cell!.createAndFill({ alignment: "left" })!
    );
    return schema.nodes.table_row!.create(null, cells);
  });
  const table = schema.nodes.table!.create(null, [headerRow, ...rows]);
  return schema.nodes.doc!.create(null, [
    table,
    schema.nodes.paragraph!.createAndFill()!
  ]);
}

function tableCellPosition(doc: ProseMirrorNode, row: number, column: number) {
  const table = doc.firstChild!;
  const map = TableMap.get(table);
  return 1 + map.map[row * map.width + column]!;
}

function stateAtCell(
  doc: ProseMirrorNode,
  row: number,
  column: number
): EditorState {
  const cellPos = tableCellPosition(doc, row, column);
  return EditorState.create({
    schema,
    doc,
    selection: TextSelection.create(doc, cellPos + 2)
  });
}

function run(command: Command, state: EditorState) {
  let nextState = state;
  const handled = command(state, (transaction) => {
    nextState = state.apply(transaction);
  });
  return { handled, state: nextState };
}

describe("markdown table toolbar commands", () => {
  it("exposes safe row operations for the GFM header/body structure", () => {
    const doc = createDocument(2, 2);
    const header = getMarkdownTableToolbarState(stateAtCell(doc, 0, 0));
    const body = getMarkdownTableToolbarState(stateAtCell(doc, 1, 0));
    const onlyBody = getMarkdownTableToolbarState(
      stateAtCell(createDocument(1, 2), 1, 0)
    );

    expect(header).toMatchObject({
      tablePos: 0,
      rowFrom: 0,
      rowTo: 1,
      rowCount: 3,
      canInsertRowBefore: false,
      canDeleteRow: false
    });
    expect(body).toMatchObject({
      rowFrom: 1,
      rowTo: 2,
      canInsertRowBefore: true,
      canDeleteRow: true
    });
    expect(onlyBody?.canDeleteRow).toBe(false);
  });

  it("never deletes the header or the final body row", () => {
    const doc = createDocument(2, 2);
    const headerResult = run(
      deleteMarkdownTableRow,
      stateAtCell(doc, 0, 0)
    );
    const bodyResult = run(
      deleteMarkdownTableRow,
      stateAtCell(doc, 1, 0)
    );
    const finalBodyResult = run(
      deleteMarkdownTableRow,
      stateAtCell(createDocument(1, 2), 1, 0)
    );

    expect(headerResult.handled).toBe(false);
    expect(bodyResult.handled).toBe(true);
    expect(bodyResult.state.doc.firstChild?.childCount).toBe(2);
    expect(finalBodyResult.handled).toBe(false);
  });

  it("deletes a selected column but preserves the final column", () => {
    const twoColumnResult = run(
      deleteMarkdownTableColumn,
      stateAtCell(createDocument(2, 2), 1, 0)
    );
    const oneColumnResult = run(
      deleteMarkdownTableColumn,
      stateAtCell(createDocument(2, 1), 1, 0)
    );

    expect(twoColumnResult.handled).toBe(true);
    expect(twoColumnResult.state.doc.firstChild?.firstChild?.childCount).toBe(1);
    expect(oneColumnResult.handled).toBe(false);
  });

  it("aligns the entire Markdown column from any cell", () => {
    const initial = stateAtCell(createDocument(2, 2), 1, 1);
    const result = run(setMarkdownTableColumnAlignment("center"), initial);
    const table = result.state.doc.firstChild!;

    expect(result.handled).toBe(true);
    for (let row = 0; row < table.childCount; row += 1) {
      expect(table.child(row).child(0).attrs.alignment).toBe("left");
      expect(table.child(row).child(1).attrs.alignment).toBe("center");
    }
    expect(getMarkdownTableToolbarState(result.state)?.alignment).toBe("center");
    expect(
      run(setMarkdownTableColumnAlignment("center"), result.state).handled
    ).toBe(false);
  });

  it("aligns every column covered by a cell selection", () => {
    const doc = createDocument(2, 2);
    const selection = new CellSelection(
      doc.resolve(tableCellPosition(doc, 1, 0)),
      doc.resolve(tableCellPosition(doc, 1, 1))
    );
    const state = EditorState.create({ schema, doc, selection });
    const result = run(setMarkdownTableColumnAlignment("right"), state);
    const table = result.state.doc.firstChild!;

    expect(result.handled).toBe(true);
    for (let row = 0; row < table.childCount; row += 1) {
      expect(table.child(row).child(0).attrs.alignment).toBe("right");
      expect(table.child(row).child(1).attrs.alignment).toBe("right");
    }
  });

  it("deletes the whole table while leaving the surrounding document valid", () => {
    const result = run(
      deleteMarkdownTable,
      stateAtCell(createDocument(2, 2), 1, 0)
    );

    expect(result.handled).toBe(true);
    expect(result.state.doc.childCount).toBe(1);
    expect(result.state.doc.firstChild?.type.name).toBe("paragraph");
  });
});
