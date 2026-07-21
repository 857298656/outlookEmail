import { readFile } from "node:fs/promises";

const iconPath = new URL("../src-tauri/icons/icon.png", import.meta.url);
const icon = await readFile(iconPath);
const pngSignature = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

if (!pngSignature.every((byte, index) => icon[index] === byte)) {
  throw new Error("src-tauri/icons/icon.png is not a valid PNG file");
}

const pngColorType = icon[25];
if (pngColorType !== 6) {
  throw new Error(
    `src-tauri/icons/icon.png must be RGBA (PNG color type 6), received ${pngColorType}`
  );
}

console.log("release icon validation passed (RGBA PNG)");
