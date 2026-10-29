// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { FieldIcons, type FieldHandlers, fieldsToMark } from "../src/field-icons";
import type { FieldClickAnswer } from "../src/messages";

function page(html: string): void {
  document.body.innerHTML = html;
}

/** Marks the clicks the user makes in tests. */
const USER = 4242;

const byId = (id: string) => document.getElementById(id) as HTMLInputElement;

const signIn = `
  <form action="/session">
    <input id="user" type="email" autocomplete="username">
    <input id="pass" type="password" autocomplete="current-password">
  </form>`;

beforeEach(() => {
  document.head.innerHTML = "";
  document.body.innerHTML = "";
});

describe("fieldsToMark", () => {
  it("marks a login's username with the logo and its password with a key", () => {
    page(signIn);

    expect(fieldsToMark(document)).toEqual([
      { input: byId("user"), kind: "login", icon: "logo" },
      { input: byId("pass"), kind: "login", icon: "key" },
    ]);
  });

  it("marks the username of a sign-in split over steps", () => {
    page(`<input id="user" type="email" name="identifier">`);

    expect(fieldsToMark(document)).toEqual([{ input: byId("user"), kind: "login", icon: "logo" }]);
  });

  it("marks a 2FA code field", () => {
    page(`<input id="code" type="text" autocomplete="one-time-code">`);

    expect(fieldsToMark(document)).toEqual([{ input: byId("code"), kind: "totp", icon: "logo" }]);
  });

  it("marks new password fields to generate a password", () => {
    page(`
      <form>
        <input id="user" type="email" autocomplete="username">
        <input id="new" type="password" autocomplete="new-password">
        <input id="again" type="password" autocomplete="new-password">
      </form>`);

    const marked = fieldsToMark(document);

    expect(marked).toContainEqual({ input: byId("new"), kind: "new-password", icon: "key" });
    expect(marked).toContainEqual({ input: byId("again"), kind: "new-password", icon: "key" });
    expect(marked).toContainEqual({ input: byId("user"), kind: "login", icon: "logo" });
  });

  it("leaves hidden fields alone", () => {
    page(`<div style="display: none">${signIn}</div>`);

    expect(fieldsToMark(document)).toEqual([]);
  });

  it("marks a payment form's text fields, not its selects", () => {
    page(`
      <form>
        <input id="email" type="email" autocomplete="email">
        <input id="holder" name="account-holder" autocomplete="cc-name">
        <input id="number" autocomplete="cc-number">
        <select id="month" autocomplete="cc-exp-month"><option>08</option></select>
        <input id="cvc" autocomplete="cc-csc">
      </form>`);

    const marked = fieldsToMark(document);

    expect(marked).toContainEqual({ input: byId("email"), kind: "login", icon: "logo" });
    for (const id of ["holder", "number", "cvc"]) {
      expect(marked).toContainEqual({ input: byId(id), kind: "payment", icon: "logo" });
    }
    expect(marked.some(({ input }) => input.id === "month")).toBe(false);
  });

  it("marks only payment fields in a page's frames", () => {
    page(`${signIn}<input id="number" autocomplete="cc-number">`);

    expect(fieldsToMark(document, { paymentOnly: true })).toEqual([
      { input: byId("number"), kind: "payment", icon: "logo" },
    ]);
  });
});

describe("FieldIcons", () => {
  let icons: FieldIcons;
  let handlers: FieldHandlers & {
    click: ReturnType<typeof vi.fn<FieldHandlers["click"]>>;
    connect: ReturnType<typeof vi.fn<FieldHandlers["connect"]>>;
  };

  const shadow = () => document.querySelector("keefast-fields")?.shadowRoot ?? undefined;
  const buttons = () =>
    Array.from(shadow()?.querySelectorAll<HTMLButtonElement>("button.icon") ?? []);
  const tooltip = () => shadow()?.querySelector<HTMLElement>(".tooltip") ?? undefined;
  /** Clicks as the user would: the page's own scripts can't make trusted clicks. */
  const click = async (element: HTMLElement) => {
    element.dispatchEvent(new MouseEvent("click", { bubbles: true, detail: USER }));
    await vi.waitFor(() => expect(element.getAttribute("aria-busy")).toBeNull());
  };

  beforeEach(() => {
    handlers = {
      click: vi.fn<FieldHandlers["click"]>(async () => ({ kind: "filled" }) as FieldClickAnswer),
      connect: vi.fn<FieldHandlers["connect"]>(async () => ({ ok: true }) as const),
    };
    // jsdom can't make trusted events: the user's clicks are marked instead.
    icons = new FieldIcons(document, handlers, {
      shadowMode: "open",
      byUser: (event) => (event as MouseEvent).detail === USER,
    });
  });

  afterEach(() => icons.stop());

  it("shows an icon in each field to mark, titled for what it does", () => {
    page(signIn);
    icons.start();

    expect(buttons().map((button) => button.title)).toEqual([
      "Fill with keefast",
      "Fill with keefast",
    ]);
  });

  it("asks for the palette to pick a Card from a payment field's icon", async () => {
    page(`<input id="number" autocomplete="cc-number">`);
    handlers.click.mockResolvedValue({ kind: "palette" });
    icons.start();

    expect(buttons().map((button) => button.title)).toEqual(["Fill a card with keefast"]);
    await click(buttons()[0]!);

    expect(handlers.click).toHaveBeenCalledWith("payment");
  });

  it("asks the service worker to fill when its icon is clicked", async () => {
    page(signIn);
    icons.start();

    await click(buttons()[1]!);

    expect(handlers.click).toHaveBeenCalledWith("login");
    expect(tooltip()).toBeUndefined();
  });

  it("ignores clicks the page's scripts make", () => {
    icons = new FieldIcons(document, handlers, { shadowMode: "open" });
    page(signIn);
    icons.start();

    buttons()[0]!.click();

    expect(handlers.click).not.toHaveBeenCalled();
  });

  it("says what to do when keefast is locked", async () => {
    handlers.click.mockResolvedValue({ kind: "unavailable", status: "locked" });
    page(signIn);
    icons.start();

    await click(buttons()[0]!);

    expect(tooltip()?.textContent).toContain("keefast is locked.");
    expect(tooltip()?.querySelector("button")).toBeNull();
  });

  it("offers to pair when the browser isn't paired, then carries on", async () => {
    handlers.click.mockResolvedValueOnce({ kind: "unavailable", status: "unpaired" });
    page(signIn);
    icons.start();
    await click(buttons()[0]!);

    const connect = tooltip()!.querySelector("button")!;
    expect(connect.textContent).toBe("Pair");
    connect.dispatchEvent(new MouseEvent("click", { bubbles: true, detail: USER }));

    await vi.waitFor(() => expect(handlers.click).toHaveBeenCalledTimes(2));
    expect(handlers.connect).toHaveBeenCalledOnce();
    expect(tooltip()).toBeUndefined();
  });

  it("says why when connecting is turned down", async () => {
    handlers.click.mockResolvedValue({ kind: "unavailable", status: "unpaired" });
    handlers.connect.mockResolvedValue({ ok: false, reason: "denied" });
    page(signIn);
    icons.start();
    await click(buttons()[0]!);

    tooltip()!
      .querySelector("button")!
      .dispatchEvent(new MouseEvent("click", { bubbles: true, detail: USER }));

    await vi.waitFor(() => expect(tooltip()?.textContent).toContain("Turned down in keefast."));
  });

  it("keeps the tooltip while the user clicks in it, and closes it on a click elsewhere", async () => {
    handlers.click.mockResolvedValue({ kind: "unavailable", status: "unpaired" });
    page(signIn);
    icons.start();
    await click(buttons()[0]!);
    const pointerDown = () => new Event("pointerdown", { bubbles: true, composed: true });

    tooltip()!.querySelector("button")!.dispatchEvent(pointerDown());
    expect(tooltip()).toBeDefined();

    document.body.dispatchEvent(pointerDown());
    expect(tooltip()).toBeUndefined();
  });

  it("focuses the field whose icon is clicked", async () => {
    page(signIn);
    icons.start();

    await click(buttons()[1]!);

    expect(document.activeElement).toBe(byId("pass"));
  });

  it("follows fields the page adds later", async () => {
    page(`<div id="root"></div>`);
    icons.start();
    expect(buttons()).toHaveLength(0);

    document.getElementById("root")!.innerHTML = signIn;

    await vi.waitFor(() => expect(buttons()).toHaveLength(2));
  });

  it("takes its icons off fields the page removes", () => {
    page(signIn);
    icons.start();

    document.body.innerHTML = "";
    icons.scan();

    expect(buttons()).toHaveLength(0);
  });

  it("leaves the page as it was once stopped", () => {
    page(signIn);
    icons.start();

    icons.stop();

    expect(document.querySelector("keefast-fields")).toBeNull();
  });
});
