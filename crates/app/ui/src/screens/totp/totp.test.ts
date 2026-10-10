import { describe, expect, it } from "vitest";

import { errorField, splitCode, uriSettings } from "./totp";

describe("uriSettings", () => {
  it("reads what an otpauth URI brings", () => {
    expect(uriSettings("otpauth://totp/GitHub:arthur-er?secret=JBSWY3DPEHPK3PXP&issuer=GitHub&algorithm=SHA256&digits=8&period=60")).toEqual({
      issuer: "GitHub",
      account: "arthur-er",
      digits: "8",
      period: "60",
      algorithm: "sha256",
    });
  });

  it("takes the issuer from the label when the parameters don't say", () => {
    expect(uriSettings("otpauth://totp/Proton%20Mail:me@proton.me?secret=AAAA")).toMatchObject({ issuer: "Proton Mail", account: "me@proton.me" });
  });

  it("is null for a secret key", () => {
    expect(uriSettings("JBSWY3DPEHPK3PXP")).toBeNull();
  });
});

describe("errorField", () => {
  it("puts each backend error by its control", () => {
    expect(errorField("A code has 1 to 10 digits.")).toBe("digits");
    expect(errorField("The period is 1 to 86400 seconds.")).toBe("period");
    expect(errorField("This isn't an otpauth://totp/ URI.")).toBe("secret");
    expect(errorField("The secret key isn't valid: use the letters A–Z and digits 2–7.")).toBe("secret");
  });
});

describe("splitCode", () => {
  it("splits a code in halves", () => {
    expect(splitCode("482913")).toEqual(["482", "913"]);
    expect(splitCode("1234567")).toEqual(["1234", "567"]);
  });
});
