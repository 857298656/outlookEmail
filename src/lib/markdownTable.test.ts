import {
  defaultValueCtx,
  Editor,
  editorViewCtx,
  serializerCtx
} from "@milkdown/kit/core";
import { Schema, type Node as ProseMirrorNode } from "@milkdown/kit/prose/model";
import { commonmark } from "@milkdown/kit/preset/commonmark";
import { gfm, tableSchema } from "@milkdown/kit/preset/gfm";
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
  insertMarkdownTableHeaderRowBefore,
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

const headerOnlyTableSchema = tableSchema.extendSchema(
  (previous) => (ctx) => ({
    ...previous(ctx),
    content: "table_header_row table_row*"
  })
);

const schema = new Schema({
  nodes: {
    doc: { content: "block+" },
    paragraph: { content: "text*", group: "block" },
    text: { group: "inline" },
    table: {
      ...tableSpecs.table,
      content: "table_header_row table_row*"
    },
    table_header_row: {
      ...tableSpecs.table_row,
      content: "table_header*"
    },
    table_row: {
      ...tableSpecs.table_row,
      content: "table_cell*"
    },
    table_cell: tableSpecs.table_cell,
    table_header: tableSpecs.table_header
  }
});

function createCell(
  type: "table_header" | "table_cell",
  text: string
) {
  const paragraph = schema.nodes.paragraph!.create(
    null,
    text ? schema.text(text) : undefined
  );
  return schema.nodes[type]!.createChecked(
    { alignment: "left" },
    paragraph
  );
}

function createDocument(bodyRows = 2, columns = 2) {
  const headerCells = Array.from({ length: columns }, (_, column) =>
    createCell("table_header", `H${column + 1}`)
  );
  const headerRow = schema.nodes.table_header_row!.createChecked(
    null,
    headerCells
  );
  const rows = Array.from({ length: bodyRows }, (_, row) => {
    const cells = Array.from({ length: columns }, (_, column) =>
      createCell("table_cell", `R${row + 1}C${column + 1}`)
    );
    return schema.nodes.table_row!.createChecked(null, cells);
  });
  const table = schema.nodes.table!.createChecked(null, [headerRow, ...rows]);
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
      canInsertRowBefore: true,
      canDeleteRow: true
    });
    expect(body).toMatchObject({
      rowFrom: 1,
      rowTo: 2,
      canInsertRowBefore: true,
      canDeleteRow: true
    });
    expect(onlyBody?.canDeleteRow).toBe(true);
  });

  it("inserts a new header above the first row and demotes the old header", () => {
    const result = run(
      insertMarkdownTableHeaderRowBefore,
      stateAtCell(createDocument(2, 2), 0, 0)
    );
    const table = result.state.doc.firstChild!;

    expect(result.handled).toBe(true);
    expect(table.childCount).toBe(4);
    expect(table.child(0).type.name).toBe("table_header_row");
    expect(table.child(0).textContent).toBe("");
    expect(table.child(1).type.name).toBe("table_row");
    expect(table.child(1).textContent).toBe("H1H2");
    expect(getMarkdownTableToolbarState(result.state)?.rowFrom).toBe(1);
    expect(() => result.state.doc.check()).not.toThrow();
  });

  it("promotes the next body row when deleting the header", () => {
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

    expect(headerResult.handled).toBe(true);
    expect(headerResult.state.doc.firstChild?.childCount).toBe(2);
    expect(headerResult.state.doc.firstChild?.child(0).type.name).toBe(
      "table_header_row"
    );
    expect(headerResult.state.doc.firstChild?.child(0).textContent).toBe(
      "R1C1R1C2"
    );
    expect(() => headerResult.state.doc.check()).not.toThrow();
    expect(bodyResult.handled).toBe(true);
    expect(bodyResult.state.doc.firstChild?.childCount).toBe(2);
    expect(finalBodyResult.handled).toBe(true);
    expect(finalBodyResult.state.doc.firstChild?.childCount).toBe(1);
    expect(() => finalBodyResult.state.doc.check()).not.toThrow();
  });

  it("leaves a single promoted header when deleting a two-row table header", () => {
    const result = run(
      deleteMarkdownTableRow,
      stateAtCell(createDocument(1, 2), 0, 0)
    );
    const table = result.state.doc.firstChild!;

    expect(result.handled).toBe(true);
    expect(table.childCount).toBe(1);
    expect(table.child(0).type.name).toBe("table_header_row");
    expect(table.child(0).textContent).toBe("R1C1R1C2");
    expect(() => result.state.doc.check()).not.toThrow();
  });

  it("supports inserting above and deleting a header-only table", () => {
    const initial = stateAtCell(createDocument(0, 2), 0, 0);
    const inserted = run(insertMarkdownTableHeaderRowBefore, initial);
    const insertedTable = inserted.state.doc.firstChild!;

    expect(inserted.handled).toBe(true);
    expect(insertedTable.childCount).toBe(2);
    expect(insertedTable.child(0).textContent).toBe("");
    expect(insertedTable.child(1).textContent).toBe("H1H2");
    expect(() => inserted.state.doc.check()).not.toThrow();

    const deleted = run(deleteMarkdownTableRow, initial);
    expect(deleted.handled).toBe(true);
    expect(deleted.state.doc.childCount).toBe(1);
    expect(deleted.state.doc.firstChild?.type.name).toBe("paragraph");
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

  it("parses and serializes a header-only table with the extended Milkdown schema", async () => {
    const editor = Editor.make()
      .config((ctx) => {
        ctx.set(defaultValueCtx, "| Header |\n| --- |\n");
      })
      .use(commonmark)
      .use(gfm)
      .use(headerOnlyTableSchema);

    await editor.create();
    try {
      const doc = editor.ctx.get(editorViewCtx).state.doc;
      expect(doc.firstChild?.type.name).toBe("table");
      expect(doc.firstChild?.childCount).toBe(1);
      expect(editor.ctx.get(serializerCtx)(doc)).toContain("Header");
    } finally {
      await editor.destroy();
    }
  });
});
