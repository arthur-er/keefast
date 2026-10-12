// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  fillLogin,
  fillNewPassword,
  fillTotp,
  findLoginFields,
  findNewPasswordFields,
  findTotpFields,
  pageFields,
} from "../src/forms";

function page(html: string): Document {
  document.body.innerHTML = html;
  return document;
}

function byId(id: string): HTMLInputElement {
  return document.getElementById(id) as HTMLInputElement;
}

const alice = { username: "alice", password: "s3cret" };

beforeEach(() => {
  document.head.innerHTML = "";
  document.body.innerHTML = "";
});

describe("findLoginFields", () => {
  it("finds the username and password of a plain sign-in form", () => {
    page(`
      <form action="/session">
        <input id="user" name="username" type="text">
        <input id="pass" name="password" type="password">
      </form>`);

    const fields = findLoginFields(document);

    expect(fields?.username).toBe(byId("user"));
    expect(fields?.password).toBe(byId("pass"));
  });

  it("takes the text field just before the password, not a search box before it", () => {
    page(`
      <form><input id="search" type="search"><input id="other" type="text"></form>
      <form>
        <input id="email" type="email">
        <input id="pass" type="password">
      </form>`);

    const fields = findLoginFields(document);

    expect(fields?.username).toBe(byId("email"));
    expect(fields?.password).toBe(byId("pass"));
  });

  it("prefers the field marked autocomplete=username", () => {
    page(`
      <form>
        <input id="user" type="text" autocomplete="username">
        <input id="code" type="text">
        <input id="pass" type="password">
      </form>`);

    expect(findLoginFields(document)?.username).toBe(byId("user"));
  });

  it("finds fields outside any form", () => {
    page(`<div><input id="user" type="text"><input id="pass" type="password"></div>`);

    const fields = findLoginFields(document);

    expect(fields?.username).toBe(byId("user"));
    expect(fields?.password).toBe(byId("pass"));
  });

  it("prefers the current password over a new one", () => {
    page(`
      <form>
        <input id="user" type="text">
        <input id="new" type="password" autocomplete="new-password">
        <input id="current" type="password" autocomplete="current-password">
      </form>`);

    expect(findLoginFields(document)?.password).toBe(byId("current"));
  });

  it("skips hidden, disabled and read-only fields", () => {
    page(`
      <style>.gone { display: none } .ghost { visibility: hidden }</style>
      <form>
        <input id="trap-user" type="text" class="gone">
        <input id="trap-pass" type="password" class="gone">
      </form>
      <div style="opacity: 0"><input id="faded" type="password"></div>
      <form>
        <input id="user" type="text">
        <input id="locked" type="text" readonly>
        <input id="ghost" type="password" class="ghost">
        <input id="off" type="password" disabled>
        <input id="hidden-attr" type="password" hidden>
        <input id="pass" type="password">
      </form>`);

    const fields = findLoginFields(document);

    expect(fields?.username).toBe(byId("user"));
    expect(fields?.password).toBe(byId("pass"));
  });

  describe("with the page laid out", () => {
    // jsdom doesn't lay pages out: give each field the box a browser would.
    const boxes = new Map<string, DOMRect>();
    beforeEach(() => {
      vi.spyOn(HTMLElement.prototype, "getClientRects").mockImplementation(function (
        this: HTMLElement,
      ) {
        const box = boxes.get(this.id) ?? new DOMRect(100, 100, 200, 30);
        return [box] as unknown as DOMRectList;
      });
    });
    afterEach(() => {
      vi.restoreAllMocks();
      boxes.clear();
    });

    it("skips fields moved off the page or shrunk to nothing", () => {
      page(`
        <form>
          <input id="off-user" type="text"><input id="off-pass" type="password">
        </form>
        <form>
          <input id="tiny-user" type="text"><input id="tiny-pass" type="password">
        </form>
        <form><input id="user" type="text"><input id="pass" type="password"></form>`);
      boxes.set("off-user", new DOMRect(-5000, 100, 200, 30));
      boxes.set("off-pass", new DOMRect(100, -5000, 200, 30));
      boxes.set("tiny-user", new DOMRect(100, 100, 0, 0));
      boxes.set("tiny-pass", new DOMRect(100, 100, 1, 1));

      const fields = findLoginFields(document);

      expect(fields?.username).toBe(byId("user"));
      expect(fields?.password).toBe(byId("pass"));
    });
  });

  it("finds a username-only step of a sign-in", () => {
    page(`
      <form>
        <input id="search" type="search">
        <input id="email" type="email" name="identifier">
      </form>`);

    const fields = findLoginFields(document);

    expect(fields?.username).toBe(byId("email"));
    expect(fields?.password).toBeUndefined();
  });

  it("finds a password-only step of a sign-in", () => {
    page(`<form><input id="pass" type="password"></form>`);

    const fields = findLoginFields(document);

    expect(fields?.username).toBeUndefined();
    expect(fields?.password).toBe(byId("pass"));
  });

  it("finds nothing on a page without a login form", () => {
    page(`
      <form><input type="search" name="q"><input type="text" name="comment"></form>
      <input type="checkbox">`);

    expect(findLoginFields(document)).toBeUndefined();
  });

  it("gives the form's action as where the login is submitted", () => {
    page(`
      <form action="https://login.example.org/session">
        <input type="text"><input type="password">
      </form>`);

    expect(findLoginFields(document)?.submitUrl).toBe("https://login.example.org/session");
  });

  it("doesn't take a 2FA field for a username", () => {
    page(`<form><input type="text" name="account-otp" autocomplete="one-time-code"></form>`);

    expect(findLoginFields(document)).toBeUndefined();
  });

  it("has no submit URL for fields outside a form", () => {
    page(`<input type="text"><input type="password">`);

    expect(findLoginFields(document)?.submitUrl).toBeUndefined();
  });
});

describe("fillLogin", () => {
  it("fills the fields and tells the page they changed", () => {
    page(`<form><input id="user" type="text"><input id="pass" type="password"></form>`);
    const events: string[] = [];
    for (const id of ["user", "pass"]) {
      for (const type of ["input", "change"]) {
        byId(id).addEventListener(type, () => events.push(`${id}:${type}`));
      }
    }

    const filled = fillLogin(document, location.origin, alice);

    expect(filled).toBe(true);
    expect(byId("user").value).toBe("alice");
    expect(byId("pass").value).toBe("s3cret");
    expect(events).toEqual(["user:input", "user:change", "pass:input", "pass:change"]);
  });

  it("fills only the username on a username-only step", () => {
    page(`<form><input id="email" type="email"></form>`);

    expect(fillLogin(document, location.origin, alice)).toBe(true);
    expect(byId("email").value).toBe("alice");
  });

  it("refuses to fill a page from another origin than the login was asked for", () => {
    page(`<form><input id="user" type="text"><input id="pass" type="password"></form>`);

    const filled = fillLogin(document, "https://bank.example", alice);

    expect(filled).toBe(false);
    expect(byId("user").value).toBe("");
    expect(byId("pass").value).toBe("");
  });

  it("fills nothing on a page without a login form", () => {
    page(`<form><input type="search"></form>`);

    expect(fillLogin(document, location.origin, alice)).toBe(false);
  });
});

/** A 6-digit code's fields, one for each digit. */
const digits = [0, 1, 2, 3, 4, 5]
  .map((i) => `<input id="d${i}" type="text" maxlength="1">`)
  .join("");

describe("findTotpFields", () => {
  it("finds the field marked autocomplete=one-time-code", () => {
    page(`<form><input id="code" type="text" autocomplete="one-time-code"></form>`);

    expect(findTotpFields(document)).toEqual([byId("code")]);
  });

  it.each([
    `<input id="code" type="text" name="totp">`,
    `<input id="code" type="tel" name="otp_code">`,
    `<input id="code" type="number" name="twoFactorCode">`,
    `<input id="code" type="text" placeholder="Authenticator code">`,
    `<input id="code" type="text" aria-label="6-digit verification code">`,
    `<input id="code" type="text" name="mfa">`,
    `<input id="code" type="text" name="code" maxlength="6">`,
    `<input id="code" type="text" name="token" inputmode="numeric">`,
  ])("finds a field named like a 2FA code: %s", (field) => {
    page(`<form>${field}</form>`);

    expect(findTotpFields(document)).toEqual([byId("code")]);
  });

  it.each([
    `<input type="text" name="promo_code" maxlength="6">`,
    `<input type="text" name="postal-code" maxlength="6">`,
    `<input type="text" name="code">`,
    `<input type="text" name="username">`,
    `<input type="password" name="otp">`,
    `<input type="text" name="otp" disabled>`,
    `<input type="text" name="otp" style="display: none">`,
  ])("doesn't take other fields for a 2FA code: %s", (field) => {
    page(`<form>${field}</form>`);

    expect(findTotpFields(document)).toEqual([]);
  });

  it("finds a code split into a field for each digit", () => {
    page(`
      <form>
        <p>Enter the code from your authenticator app.</p>
        <input id="search" type="text" maxlength="1">
        <div>${digits}</div>
      </form>`);

    expect(findTotpFields(document)).toEqual([0, 1, 2, 3, 4, 5].map((i) => byId(`d${i}`)));
  });

  it("finds digit fields each in a box of its own", () => {
    const boxed = Array(6).fill(`<span><input maxlength="1"></span>`);
    page(`<form>Verification code ${boxed.join("")}</form>`);

    expect(findTotpFields(document)).toHaveLength(6);
  });

  it("doesn't take digit fields that don't ask for a code for one", () => {
    page(`<form>Your card's PIN ${digits}</form>`);
    expect(findTotpFields(document)).toEqual([]);

    page(`<form>Enter the promo code ${digits}</form>`);
    expect(findTotpFields(document)).toEqual([]);
  });

  it("prefers the marked field over digit fields", () => {
    page(`
      <form>
        ${digits}
        <input id="code" type="text" autocomplete="one-time-code">
      </form>`);

    expect(findTotpFields(document)).toEqual([byId("code")]);
  });
});

describe("fillTotp", () => {
  it("fills the code and tells the page it changed", () => {
    page(`<form><input id="code" type="text" autocomplete="one-time-code"></form>`);
    const events: string[] = [];
    byId("code").addEventListener("input", () => events.push("input"));

    expect(fillTotp(document, location.origin, "123456")).toBe(true);
    expect(byId("code").value).toBe("123456");
    expect(events).toEqual(["input"]);
  });

  it("fills a digit into each of the code's digit fields", () => {
    page(`<form>Enter your 2FA code ${digits}</form>`);

    expect(fillTotp(document, location.origin, "123456")).toBe(true);
    expect([0, 1, 2, 3, 4, 5].map((i) => byId(`d${i}`).value).join("")).toBe("123456");
  });

  it("refuses to fill a page from another origin than the code was asked for", () => {
    page(`<form><input id="code" type="text" autocomplete="one-time-code"></form>`);

    expect(fillTotp(document, "https://bank.example", "123456")).toBe(false);
    expect(byId("code").value).toBe("");
  });

  it("fills nothing on a page without a 2FA field", () => {
    page(`<form><input type="text" name="username"></form>`);

    expect(fillTotp(document, location.origin, "123456")).toBe(false);
  });
});

describe("findNewPasswordFields", () => {
  it("finds the fields marked autocomplete=new-password", () => {
    page(`
      <form>
        <input id="current" type="password" autocomplete="current-password">
        <input id="new" type="password" autocomplete="new-password">
        <input id="again" type="password" autocomplete="new-password">
      </form>`);

    expect(findNewPasswordFields(document)).toEqual([byId("new"), byId("again")]);
  });

  it("finds a sign-up's password and its confirmation, unmarked", () => {
    page(`
      <form>
        <input type="email">
        <input id="pass" type="password"><input id="again" type="password">
      </form>`);

    expect(findNewPasswordFields(document)).toEqual([byId("pass"), byId("again")]);
  });

  it("finds the new password and its confirmation of a password change, unmarked", () => {
    page(`
      <form>
        <input id="current" type="password">
        <input id="new" type="password"><input id="again" type="password">
      </form>`);

    expect(findNewPasswordFields(document)).toEqual([byId("new"), byId("again")]);
  });

  it("finds a lone password field named as a new one", () => {
    page(`<form><input type="email"><input id="pass" type="password" name="new_password"></form>`);

    expect(findNewPasswordFields(document)).toEqual([byId("pass")]);
  });

  it("doesn't take a sign-in's password for a new one", () => {
    page(`<form><input type="text"><input type="password" name="password"></form>`);

    expect(findNewPasswordFields(document)).toEqual([]);
  });
});

describe("fillNewPassword", () => {
  it("fills the new password and its confirmation", () => {
    page(`
      <form>
        <input id="current" type="password" autocomplete="current-password">
        <input id="new" type="password" autocomplete="new-password">
        <input id="again" type="password" autocomplete="new-password">
      </form>`);

    expect(fillNewPassword(document, location.origin, "G3n!")).toBe(true);
    expect([byId("current"), byId("new"), byId("again")].map((f) => f.value)).toEqual([
      "",
      "G3n!",
      "G3n!",
    ]);
  });

  it("refuses to fill a page from another origin than the password was made for", () => {
    page(`<form><input id="new" type="password" autocomplete="new-password"></form>`);

    expect(fillNewPassword(document, "https://bank.example", "G3n!")).toBe(false);
    expect(byId("new").value).toBe("");
  });

  it("fills nothing on a page without new password fields", () => {
    page(`<form><input type="text"><input type="password"></form>`);

    expect(fillNewPassword(document, location.origin, "G3n!")).toBe(false);
  });
});

describe("pageFields", () => {
  it("says what the page has to fill", () => {
    page(`<form><input type="text" autocomplete="one-time-code"></form>`);
    expect(pageFields(document)).toEqual({
      login: false,
      totp: true,
      newPassword: false,
      payment: false,
    });

    page(`<form><input type="email"><input type="password" autocomplete="new-password"></form>`);
    expect(pageFields(document)).toEqual({
      login: true,
      totp: false,
      newPassword: true,
      payment: false,
    });

    page(`<form><input autocomplete="cc-number"><input autocomplete="cc-csc"></form>`);
    expect(pageFields(document).payment).toBe(true);
  });
});
