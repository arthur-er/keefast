// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from "vitest";
import { submitsLogin, submittedLogin } from "../src/capture";

/** Lays out the page and types `values` into its fields, by id. */
function page(html: string, values: Record<string, string> = {}): Document {
  document.body.innerHTML = html;
  for (const [id, value] of Object.entries(values)) {
    (document.getElementById(id) as HTMLInputElement).value = value;
  }
  return document;
}

function form(id = "form"): HTMLFormElement {
  return document.getElementById(id) as HTMLFormElement;
}

beforeEach(() => {
  document.body.innerHTML = "";
});

describe("a sign-in", () => {
  it("is the username and password the user typed", () => {
    page(
      `<form id="form">
        <input id="user" type="text"><input id="pass" type="password">
      </form>`,
      { user: "alice", pass: "s3cret" },
    );

    expect(submittedLogin(form())).toEqual({ username: "alice", password: "s3cret" });
  });

  it("takes the username field marked as such over other text fields", () => {
    page(
      `<form id="form">
        <input id="user" type="email" autocomplete="username">
        <input id="code" type="text"><input id="pass" type="password">
      </form>`,
      { user: "alice@example.com", code: "1234", pass: "s3cret" },
    );

    expect(submittedLogin(form())?.username).toBe("alice@example.com");
  });

  it("takes the text field just before the password", () => {
    page(
      `<form id="form">
        <input id="company" type="text"><input id="user" type="text">
        <input id="pass" type="password">
      </form>`,
      { company: "acme", user: "alice", pass: "s3cret" },
    );

    expect(submittedLogin(form())?.username).toBe("alice");
  });

  it("may have no username", () => {
    page(`<form id="form"><input id="pass" type="password"></form>`, { pass: "s3cret" });

    expect(submittedLogin(form())).toEqual({ username: "", password: "s3cret" });
  });

  it("ignores text fields after the password, such as a one-time code", () => {
    page(
      `<form id="form"><input id="pass" type="password"><input id="otp" type="text"></form>`,
      { pass: "s3cret", otp: "123456" },
    );

    expect(submittedLogin(form())?.username).toBe("");
  });

  it("is only what is in the submitted form", () => {
    page(
      `<form id="search"><input id="q" type="text"></form>
      <form id="form"><input id="user" type="text"><input id="pass" type="password"></form>`,
      { q: "shoes", user: "alice", pass: "s3cret" },
    );

    expect(submittedLogin(form())?.username).toBe("alice");
    expect(submittedLogin(form("search"))).toBeUndefined();
  });

  it("is found among fields outside any form", () => {
    page(
      `<form id="form"><input id="q" type="text"><input id="other" type="password"></form>
      <input id="user" type="text"><input id="pass" type="password">`,
      { q: "shoes", other: "x", user: "alice", pass: "s3cret" },
    );

    expect(submittedLogin(document)).toEqual({ username: "alice", password: "s3cret" });
  });

  it("is nothing without a password typed", () => {
    page(
      `<form id="form"><input id="user" type="text"><input id="pass" type="password"></form>`,
      { user: "alice" },
    );

    expect(submittedLogin(form())).toBeUndefined();
  });
});

describe("a sign-up", () => {
  it("is the new password, typed twice", () => {
    page(
      `<form id="form">
        <input id="user" type="email"><input id="new" type="password">
        <input id="again" type="password">
      </form>`,
      { user: "alice@example.com", new: "fresh-one", again: "fresh-one" },
    );

    expect(submittedLogin(form())).toEqual({
      username: "alice@example.com",
      password: "fresh-one",
    });
  });

  it("is the field marked as a new password", () => {
    page(
      `<form id="form">
        <input id="user" type="text"><input id="new" type="password" autocomplete="new-password">
      </form>`,
      { user: "alice", new: "fresh-one" },
    );

    expect(submittedLogin(form())).toEqual({ username: "alice", password: "fresh-one" });
  });

  it("is nothing when the new password was mistyped the second time", () => {
    page(
      `<form id="form">
        <input id="new" type="password" autocomplete="new-password">
        <input id="again" type="password" autocomplete="new-password">
      </form>`,
      { new: "fresh-one", again: "fresh-onw" },
    );

    expect(submittedLogin(form())).toBeUndefined();
  });
});

describe("a password change", () => {
  it("is the new password, with the current one it replaces", () => {
    page(
      `<form id="form">
        <input id="current" type="password"><input id="new" type="password">
        <input id="again" type="password">
      </form>`,
      { current: "old-one", new: "fresh-one", again: "fresh-one" },
    );

    expect(submittedLogin(form())).toEqual({
      username: "",
      password: "fresh-one",
      current: "old-one",
    });
  });

  it("may have no confirmation", () => {
    page(
      `<form id="form"><input id="current" type="password"><input id="new" type="password"></form>`,
      { current: "old-one", new: "fresh-one" },
    );

    expect(submittedLogin(form())).toMatchObject({ password: "fresh-one", current: "old-one" });
  });

  it("follows the fields' autocomplete marks over their order", () => {
    page(
      `<form id="form">
        <input id="new" type="password" autocomplete="new-password">
        <input id="current" type="password" autocomplete="current-password">
      </form>`,
      { new: "fresh-one", current: "old-one" },
    );

    expect(submittedLogin(form())).toMatchObject({ password: "fresh-one", current: "old-one" });
  });

  it("takes the username from a hidden username field, as change forms have", () => {
    page(
      `<form id="form">
        <input id="user" type="text" autocomplete="username" style="display: none">
        <input id="current" type="password"><input id="new" type="password">
        <input id="again" type="password">
      </form>`,
      { user: "alice", current: "old-one", new: "fresh-one", again: "fresh-one" },
    );

    expect(submittedLogin(form())?.username).toBe("alice");
  });

  it("is nothing when the new password was mistyped the second time", () => {
    page(
      `<form id="form">
        <input id="current" type="password"><input id="new" type="password">
        <input id="again" type="password">
      </form>`,
      { current: "old-one", new: "fresh-one", again: "fresh-onw" },
    );

    expect(submittedLogin(form())).toBeUndefined();
  });

  it("is a plain sign-in when the new password is the current one", () => {
    page(
      `<form id="form">
        <input id="current" type="password" autocomplete="current-password">
        <input id="new" type="password" autocomplete="new-password">
      </form>`,
      { current: "same-one", new: "same-one" },
    );

    expect(submittedLogin(form())).toEqual({ username: "", password: "same-one" });
  });

  it("ignores password fields left empty", () => {
    page(
      `<form id="form">
        <input id="user" type="text"><input id="pass" type="password">
        <input id="unused" type="password">
      </form>`,
      { user: "alice", pass: "s3cret" },
    );

    expect(submittedLogin(form())).toEqual({ username: "alice", password: "s3cret" });
  });
});

describe("submitsLogin", () => {
  const clicked = (html: string) => {
    document.body.innerHTML = `<form>${html}</form>`;
    return submitsLogin(document.getElementById("clicked")!);
  };

  it("is true of a form's submit buttons", () => {
    expect(clicked(`<button id="clicked">Go</button>`)).toBe(true);
    expect(clicked(`<input id="clicked" type="submit" value="Go">`)).toBe(true);
  });

  it("is true of a scripted button that says it signs in", () => {
    expect(clicked(`<button id="clicked" type="button">Sign in</button>`)).toBe(true);
    expect(clicked(`<div id="clicked" role="button">Log in</div>`)).toBe(true);
    expect(clicked(`<button id="clicked" type="button" aria-label="Continue">→</button>`)).toBe(
      true,
    );
  });

  it("is false of buttons that don't submit, like showing the password", () => {
    expect(clicked(`<button id="clicked" type="button">Show password</button>`)).toBe(false);
    expect(clicked(`<button id="clicked" type="button">Forgot password?</button>`)).toBe(false);
    expect(clicked(`<button id="clicked" type="button" aria-label="Show">👁</button>`)).toBe(
      false,
    );
  });

  it("is true of what is inside a submit button", () => {
    document.body.innerHTML = `<form><button><span id="clicked">Sign in</span></button></form>`;

    expect(submitsLogin(document.getElementById("clicked")!)).toBe(true);
  });

  it("is false of what isn't a button", () => {
    expect(clicked(`<input id="clicked" type="text">`)).toBe(false);
    expect(clicked(`<p id="clicked">Sign in</p>`)).toBe(false);
  });
});
