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
        remark: "main",
        provider: "graph"
      }
    ]);
  });

  it("ignores comments and invalid rows", () => {
    const rows = parseAccountRows("# comment\nnot-email\nok@example.com,pass");
    expect(rows).toHaveLength(1);
    expect(rows[0].email).toBe("ok@example.com");
  });

  it("detects common mailbox providers from email domains", () => {
    const rows = parseAccountRows("person@gmail.com\nuser@qq.com----code\nalias@foxmail.com----code\nmail@163.com----secret");
    expect(rows.map((row) => row.provider)).toEqual(["gmail", "qq", "qq", "netease_163"]);
  });

  it("supports explicit provider fields without breaking legacy columns", () => {
    expect(parseAccountRows("provider=qq----user@example.com----auth")[0]).toMatchObject({
      email: "user@example.com",
      password: "auth",
      provider: "qq"
    });
    expect(parseAccountRows("netease_163----user@custom.test----secret")[0]).toMatchObject({
      email: "user@custom.test",
      password: "secret",
      provider: "netease_163"
    });
  });
});
