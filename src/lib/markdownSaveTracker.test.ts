import { describe, expect, it } from "vitest";
import { MarkdownSaveTracker } from "./markdownSaveTracker";

describe("MarkdownSaveTracker", () => {
  it("invalidates an in-flight save when the active document changes", () => {
    const tracker = new MarkdownSaveTracker();
    tracker.activate(1);
    const token = tracker.capture();

    expect(token).not.toBeNull();
    expect(tracker.isCurrent(token!)).toBe(true);

    tracker.activate(2);
    expect(tracker.isCurrent(token!)).toBe(false);
  });

  it("invalidates an in-flight save when more content is edited", () => {
    const tracker = new MarkdownSaveTracker();
    tracker.activate(1);
    const token = tracker.capture();

    tracker.markChanged();
    expect(tracker.isCurrent(token!)).toBe(false);
    expect(tracker.capture()?.revision).toBeGreaterThan(token!.revision);
  });
});
