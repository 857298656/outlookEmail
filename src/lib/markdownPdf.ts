export const A4_PDF_PAGE = {
  widthMm: 210,
  heightMm: 297,
  marginMm: 15
} as const;

export type MarkdownPdfSlice = {
  sourceY: number;
  sourceHeight: number;
  imageHeightMm: number;
};

export type MarkdownPdfLayout = {
  pageWidthMm: number;
  pageHeightMm: number;
  marginMm: number;
  imageWidthMm: number;
  slices: MarkdownPdfSlice[];
};

export function calculateMarkdownPdfLayout(
  canvasWidth: number,
  canvasHeight: number
): MarkdownPdfLayout {
  if (canvasWidth <= 0 || canvasHeight <= 0) {
    throw new Error("PDF export canvas must have a positive size");
  }

  const { widthMm, heightMm, marginMm } = A4_PDF_PAGE;
  const imageWidthMm = widthMm - marginMm * 2;
  const contentHeightMm = heightMm - marginMm * 2;
  const pixelsPerMm = canvasWidth / imageWidthMm;
  const maxSliceHeight = Math.max(1, Math.floor(contentHeightMm * pixelsPerMm));
  const slices: MarkdownPdfSlice[] = [];

  for (let sourceY = 0; sourceY < canvasHeight; sourceY += maxSliceHeight) {
    const sourceHeight = Math.min(maxSliceHeight, canvasHeight - sourceY);
    slices.push({
      sourceY,
      sourceHeight,
      imageHeightMm: sourceHeight / pixelsPerMm
    });
  }

  return {
    pageWidthMm: widthMm,
    pageHeightMm: heightMm,
    marginMm,
    imageWidthMm,
    slices
  };
}
