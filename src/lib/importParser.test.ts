import { describe, expect, it } from "vitest";
import { parseAccountRows } from "./importParser";

describe("parseAccountRows", () => {
  it("parses legacy outlook import rows", () => {
    const rows = parseAccountRows("User@Example.com----pass----client----refresh----main");
    expect(rows).toEqual([
      {
        email: "user@example.com",
        password: "pass",
        client_id: "client",
        refresh_token: "refresh",
        remark: "main"
      }
    ]);
  });

  it("ignores comments and invalid rows", () => {
    const rows = parseAccountRows("# comment\nnot-email\nok@example.com,pass");
    expect(rows).toHaveLength(1);
    expect(rows[0].email).toBe("ok@example.com");
  });
});
