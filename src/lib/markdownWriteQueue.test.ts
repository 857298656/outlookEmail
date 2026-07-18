import { describe, expect, it } from "vitest";
import { MarkdownWriteQueue } from "./markdownWriteQueue";

describe("MarkdownWriteQueue", () => {
  it("serializes document writes in enqueue order", async () => {
    const queue = new MarkdownWriteQueue();
    const events: string[] = [];
    let releaseFirst: (() => void) | undefined;

    const first = queue.enqueue(
      () =>
        new Promise<string>((resolve) => {
          events.push("first:start");
          releaseFirst = () => {
            events.push("first:end");
            resolve("first");
          };
        })
    );
    const second = queue.enqueue(async () => {
      events.push("second:start");
      return "second";
    });

    await Promise.resolve();
    expect(events).toEqual(["first:start"]);
    releaseFirst?.();
    await expect(first).resolves.toBe("first");
    await expect(second).resolves.toBe("second");
    expect(events).toEqual(["first:start", "first:end", "second:start"]);
  });

  it("continues after a failed write", async () => {
    const queue = new MarkdownWriteQueue();
    const failed = queue.enqueue(async () => {
      throw new Error("save failed");
    });
    const recovered = queue.enqueue(async () => "saved");

    await expect(failed).rejects.toThrow("save failed");
    await expect(recovered).resolves.toBe("saved");
  });
});
