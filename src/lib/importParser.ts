export type ParsedAccount = {
  email: string;
  password: string;
  client_id: string;
  refresh_token: string;
  remark: string;
};

export function parseAccountRows(raw: string): ParsedAccount[] {
  return raw
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0 && !line.startsWith("#"))
    .map((line) => splitLine(line))
    .filter((parts) => parts[0]?.includes("@"))
    .map((parts) => ({
      email: parts[0].trim().toLowerCase(),
      password: parts[1]?.trim() ?? "",
      client_id: parts[2]?.trim() ?? "",
      refresh_token: parts[3]?.trim() ?? "",
      remark: parts[4]?.trim() ?? ""
    }));
}

function splitLine(line: string): string[] {
  const delimiter = ["----", "|||", "\t", ","].find((item) => line.includes(item));
  return delimiter ? line.split(delimiter) : [line];
}
