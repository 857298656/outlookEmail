export class MarkdownWriteQueue {
  private tail: Promise<void> = Promise.resolve();

  enqueue<T>(write: () => Promise<T>): Promise<T> {
    const result = this.tail.then(write, write);
    this.tail = result.then(
      () => undefined,
      () => undefined
    );
    return result;
  }
}
