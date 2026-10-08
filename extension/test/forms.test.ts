// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from "vitest";
import { findLoginFields, fillLogin } from "../src/forms";

function page(html: string): Document {
  document.body.innerHTML = html;
  return document;
}

function byId(id: string): HTMLInputElement {
  return document.getElementById(id) as HTMLInputElement;
}

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

    const filled = fillLogin(document, location.origin, "alice", "s3cret");

    expect(filled).toBe(true);
    expect(byId("user").value).toBe("alice");
    expect(byId("pass").value).toBe("s3cret");
    expect(events).toEqual(["user:input", "user:change", "pass:input", "pass:change"]);
  });

  it("fills only the username on a username-only step", () => {
    page(`<form><input id="email" type="email"></form>`);

    expect(fillLogin(document, location.origin, "alice", "s3cret")).toBe(true);
    expect(byId("email").value).toBe("alice");
  });

  it("refuses to fill a page from another origin than the login was asked for", () => {
    page(`<form><input id="user" type="text"><input id="pass" type="password"></form>`);

    const filled = fillLogin(document, "https://bank.example", "alice", "s3cret");

    expect(filled).toBe(false);
    expect(byId("user").value).toBe("");
    expect(byId("pass").value).toBe("");
  });

  it("fills nothing on a page without a login form", () => {
    page(`<form><input type="search"></form>`);

    expect(fillLogin(document, location.origin, "alice", "s3cret")).toBe(false);
  });
});
