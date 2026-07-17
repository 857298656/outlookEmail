export type MarkdownSaveToken = {
  documentId: number;
  revision: number;
};

export class MarkdownSaveTracker {
  private documentId: number | undefined;
  private revision = 0;

  activate(documentId: number | undefined) {
    this.documentId = documentId;
    this.revision += 1;
  }

  markChanged() {
    this.revision += 1;
  }

  capture(): MarkdownSaveToken | null {
    return this.documentId === undefined
      ? null
      : { documentId: this.documentId, revision: this.revision };
  }

  isCurrent(token: MarkdownSaveToken) {
    return token.documentId === this.documentId && token.revision === this.revision;
  }
}
