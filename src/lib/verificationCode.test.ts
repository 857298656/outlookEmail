import { describe, expect, it } from "vitest";
import { extractVerificationCode } from "./verificationCode";

describe("extractVerificationCode", () => {
  it("extracts a Chinese numeric verification code", () => {
    expect(
      extractVerificationCode({
        subject: "你的 OpenAI 临时验证码",
        body_preview: "输入此临时验证码以继续：708604 如果并非你本人尝试创建账户，请忽略此电子邮件。"
      })
    ).toBe("708604");
  });

  it("extracts an English verification code", () => {
    expect(
      extractVerificationCode({
        subject: "Your verification code",
        body_preview: "Use verification code AB-1234 to continue signing in."
      })
    ).toBe("AB1234");
  });

  it("returns null for regular email text", () => {
    expect(
      extractVerificationCode({
        subject: "Welcome to Claude. Let's get you set up.",
        body_preview: "Your step-by-step list to make Claude work better with you."
      })
    ).toBeNull();
  });
});
