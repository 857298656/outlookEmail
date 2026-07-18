import { describe, expect, it } from "vitest";
import { calculateMarkdownPdfLayout } from "./markdownPdf";

describe("Markdown A4 PDF layout", () => {
  it("uses a full portrait A4 page for short content", () => {
    const layout = calculateMarkdownPdfLayout(1200, 240);

    expect(layout.pageWidthMm).toBe(210);
    expect(layout.pageHeightMm).toBe(297);
    expect(layout.marginMm).toBe(15);
    expect(layout.imageWidthMm).toBe(180);
    expect(layout.slices).toHaveLength(1);
    expect(layout.slices[0]?.imageHeightMm).toBeCloseTo(36);
  });

  it("splits tall content into consecutive A4 page slices", () => {
    const layout = calculateMarkdownPdfLayout(1200, 5000);
    const coveredHeight = layout.slices.reduce(
      (total, slice) => total + slice.sourceHeight,
      0
    );

    expect(layout.slices.length).toBeGreaterThan(1);
    expect(layout.slices[0]?.sourceY).toBe(0);
    expect(coveredHeight).toBe(5000);
    layout.slices.forEach((slice, index) => {
      if (index > 0) {
        const previous = layout.slices[index - 1]!;
        expect(slice.sourceY).toBe(previous.sourceY + previous.sourceHeight);
      }
      expect(slice.imageHeightMm).toBeLessThanOrEqual(267);
    });
  });

  it("rejects an empty canvas", () => {
    expect(() => calculateMarkdownPdfLayout(0, 100)).toThrow(
      "PDF export canvas must have a positive size"
    );
  });
});
