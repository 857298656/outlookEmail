import type { ImportTempEmailsInput } from "../types";

export const tempEmailImportChunkSize = 20;

type ImportRecord = {
  line: string;
  cloudflareHeader?: string;
};

function isCloudflareHeader(line: string) {
  return /^\[\s*cloudflare(?:\s*:[^\]]*)?\s*\]$/i.test(line);
}

export function buildTempEmailImportChunks(
  raw: string,
  provider: ImportTempEmailsInput["provider"],
  chunkSize = tempEmailImportChunkSize
) {
  const size = Math.max(1, Math.floor(chunkSize));
  const lines = raw
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);

  if (provider !== "cloudflare") {
    return Array.from({ length: Math.ceil(lines.length / size) }, (_, index) =>
      lines.slice(index * size, (index + 1) * size).join("\n")
    ).filter(Boolean);
  }

  let cloudflareHeader: string | undefined;
  const records: ImportRecord[] = [];
  for (const line of lines) {
    if (isCloudflareHeader(line)) {
      cloudflareHeader = line;
      continue;
    }
    records.push({ line, cloudflareHeader });
  }

  return Array.from({ length: Math.ceil(records.length / size) }, (_, index) => {
    const chunk = records.slice(index * size, (index + 1) * size);
    const output: string[] = [];
    let activeHeader: string | undefined;
    for (const record of chunk) {
      if (record.cloudflareHeader !== activeHeader) {
        activeHeader = record.cloudflareHeader;
        if (activeHeader) output.push(activeHeader);
      }
      output.push(record.line);
    }
    return output.join("\n");
  }).filter(Boolean);
}
