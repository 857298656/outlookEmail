import { commandsCtx } from "@milkdown/kit/core";
import {
  addRowAfterCommand,
  addRowBeforeCommand
} from "@milkdown/kit/preset/gfm";
import { keymap } from "@milkdown/kit/prose/keymap";
import { $prose } from "@milkdown/kit/utils";
import {
  deleteMarkdownTableRow,
  getMarkdownTableToolbarState,
  insertMarkdownTableHeaderRowBefore
} from "./markdownTable";

export const markdownTableRowShortcuts = $prose((ctx) =>
  keymap({
    "Alt-ArrowUp": (state, dispatch, view) => {
      const tableState = getMarkdownTableToolbarState(state);
      if (!tableState) return false;
      if (tableState.rowFrom === 0) {
        return insertMarkdownTableHeaderRowBefore(state, dispatch, view);
      }
      return ctx.get(commandsCtx).call(addRowBeforeCommand.key);
    },
    "Alt-ArrowDown": (state) => {
      if (!getMarkdownTableToolbarState(state)) return false;
      return ctx.get(commandsCtx).call(addRowAfterCommand.key);
    },
    Delete: deleteMarkdownTableRow
  })
);
