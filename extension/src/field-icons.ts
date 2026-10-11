// keefast's icon in sign-in fields, as KeePassXC-Browser shows its own: the logo in a login's
// username field and a 2FA code field, a key in password fields, and the logo in a payment
// form's fields (ADR 0010). Clicking it fills the site's only Entry, or opens the command
// palette, which a payment field's always opens, to pick a Card; when keefast isn't ready, a tooltip says why, and
// offers to connect the browser when it isn't paired. The icons show nothing secret, and are
// drawn in a closed shadow root over the page, which they leave as it is (ADR 0007).
import { findLoginFields, findNewPasswordFields, findTotpFields, shown } from "./forms";
import { findPaymentFields } from "./payment";
import type { FieldClickAnswer, FieldKind } from "./messages";
import type { FailureReason, Status } from "./keefast";

/** A field keefast shows its icon in. */
export interface MarkedField {
  input: HTMLInputElement;
  kind: FieldKind;
  icon: "logo" | "key";
}

/** The smallest a field can be, in pixels each way, to have room for the icon. */
const MIN_FIELD = { width: 60, height: 16 };
const ICON_SIZE = 20;
/** How far the icon sits from the field's right edge. */
const ICON_INSET = 8;
/** How long the page may keep changing before the fields are looked for again. */
const RESCAN_DELAY_MS = 250;

/**
 * The fields to show keefast's icon in: a login's username and password, a 2FA code field
 * (the first, when split over digit fields), new password fields, which the icon offers a
 * generated password for, and a payment form's text fields. With `paymentOnly`, as in a page's
 * frames, only payment fields.
 */
export function fieldsToMark(document: Document, { paymentOnly = false } = {}): MarkedField[] {
  const marked = new Map<HTMLInputElement, MarkedField>();
  const view = document.defaultView;
  const payment = findPaymentFields(document).flatMap(({ element }) =>
    // A select has no room for the icon.
    view && element instanceof view.HTMLInputElement ? [element] : [],
  );
  const markPayment = () => {
    // Over any other kind a field was taken for, such as a cardholder's name for a username.
    for (const input of payment) marked.set(input, { input, kind: "payment", icon: "logo" });
  };
  if (paymentOnly) {
    markPayment();
    return [...marked.values()].filter(({ input }) => roomy(input));
  }
  const login = findLoginFields(document);
  if (login?.username)
    marked.set(login.username, { input: login.username, kind: "login", icon: "logo" });
  if (login?.password)
    marked.set(login.password, { input: login.password, kind: "login", icon: "key" });
  const [totp] = findTotpFields(document);
  if (totp) marked.set(totp, { input: totp, kind: "totp", icon: "logo" });
  for (const input of findNewPasswordFields(document)) {
    marked.set(input, { input, kind: "new-password", icon: "key" });
  }
  markPayment();
  return [...marked.values()].filter(({ input }) => roomy(input));
}

/** Whether a field has room for the icon. Unless the page isn't laid out (as in tests). */
function roomy(input: HTMLInputElement): boolean {
  const box = input.getClientRects()[0];
  return !box || (box.width >= MIN_FIELD.width && box.height >= MIN_FIELD.height);
}

const TITLES: Record<FieldKind, string> = {
  login: "Fill with keefast",
  totp: "Fill the code with keefast",
  "new-password": "Generate a password with keefast",
  payment: "Fill a card with keefast",
};

/** The slanted keyhole of keefast's logo, as one colour. */
const LOGO_GLYPH = `<svg viewBox="3 9 48 46" aria-hidden="true"><g transform="skewX(-12) translate(7 0)" fill="currentColor"><circle cx="34" cy="25" r="10"/><path d="M29 30h10l3 20H26z"/></g><path d="M10 22h9M7 31h10M10 40h7" stroke="currentColor" stroke-width="3.5" stroke-linecap="round"/></svg>`;
const KEY_GLYPH = `<svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="8" cy="15" r="4"/><path d="m10.85 12.15 8.65-8.65M18 5l2 2M15 8l2 2"/></svg>`;

const STYLE = `
:host { all: initial; }
button.icon {
  all: unset;
  position: fixed;
  box-sizing: border-box;
  width: ${ICON_SIZE}px;
  height: ${ICON_SIZE}px;
  padding: 1px;
  border-radius: 4px;
  color: #8a8f98;
  cursor: pointer;
  pointer-events: auto;
  transition: color 120ms ease-out, transform 120ms ease-out, opacity 120ms ease-out;
}
button.icon:hover, button.icon:focus-visible { color: #c026d3; }
button.icon:focus-visible { outline: 2px solid #c026d3; outline-offset: 1px; }
button.icon:active { transform: scale(0.92); }
button.icon[aria-busy="true"] { opacity: 0.5; cursor: progress; }
button.icon svg { display: block; width: 100%; height: 100%; }
.tooltip {
  position: fixed;
  box-sizing: border-box;
  max-width: 260px;
  padding: 10px 12px;
  border: 1px solid #e4e4e7;
  border-radius: 8px;
  background: #ffffff;
  color: #18181b;
  box-shadow: 0 4px 12px rgb(0 0 0 / 0.12);
  font: 13px/1.4 system-ui, -apple-system, "Segoe UI", sans-serif;
  pointer-events: auto;
  animation: in 140ms ease-out;
}
.tooltip p { margin: 0; }
.tooltip .title { font-weight: 600; }
.tooltip .detail { margin-top: 2px; color: #71717a; }
.tooltip button {
  all: unset;
  margin-top: 8px;
  padding: 5px 10px;
  border-radius: 6px;
  background: #c026d3;
  color: #ffffff;
  font-weight: 500;
  cursor: pointer;
}
.tooltip button:hover { background: #a21caf; }
.tooltip button:focus-visible { outline: 2px solid #c026d3; outline-offset: 2px; }
.tooltip button[disabled] { opacity: 0.6; cursor: progress; }
@media (prefers-color-scheme: dark) {
  .tooltip { background: #18181b; color: #fafafa; border-color: #27272a; }
  .tooltip .detail { color: #a1a1aa; }
}
@keyframes in { from { opacity: 0; transform: translateY(-2px); } }
@media (prefers-reduced-motion: reduce) {
  button.icon, .tooltip { transition: none; animation: none; }
}
`;

/** What the icons ask of the service worker. */
export interface FieldHandlers {
  click(field: FieldKind): Promise<FieldClickAnswer>;
  connect(): Promise<{ ok: true } | { ok: false; reason: FailureReason }>;
}

type Unavailable = Exclude<Status, "paired"> | FailureReason;

/** What keefast not being ready means for the user, and what they can do. */
function explain(status: Unavailable): [title: string, detail?: string] {
  switch (status) {
    case "unpaired":
      return ["Connect this browser to keefast.", "You approve the Pairing in keefast."];
    case "not-running":
      return ["keefast isn't running.", "Start keefast and unlock it."];
    case "locked":
      return ["keefast is locked.", "Unlock it, then try again."];
    case "denied":
      return ["Turned down in keefast."];
    case "failed":
      return ["keefast can't fill this page right now.", "Try again."];
  }
}

export class FieldIcons {
  private host?: HTMLElement;
  private shadow?: ShadowRoot;
  private icons = new Map<HTMLInputElement, { button: HTMLButtonElement; field: MarkedField }>();
  private tooltip?: { element: HTMLElement; anchor: HTMLButtonElement };
  private observer?: MutationObserver;
  /** Moves the icons when their fields move or resize without the page changing. */
  private resizes?: ResizeObserver;
  private rescanTimer?: ReturnType<typeof setTimeout>;
  private frame?: number;

  constructor(
    private readonly document: Document,
    private readonly handlers: FieldHandlers,
    private readonly options: {
      /** Tests look inside; pages never can. */
      shadowMode?: ShadowRootMode;
      /** Whether the user did this: the page's own scripts can click too. */
      byUser?: (event: Event) => boolean;
      /** Marks only payment fields, as in a page's frames. */
      paymentOnly?: boolean;
    } = {},
  ) {}

  private byUser(event: Event): boolean {
    return (this.options.byUser ?? ((event) => event.isTrusted))(event);
  }

  /** Shows the icons, and keeps them on the page's fields as it changes. */
  start(): void {
    if (this.host) return;
    const host = this.document.createElement("keefast-fields");
    host.style.cssText =
      "all: initial !important; position: fixed !important; inset: 0 !important; " +
      "pointer-events: none !important; z-index: 2147483646 !important;";
    this.shadow = host.attachShadow({ mode: this.options.shadowMode ?? "closed" });
    const style = this.document.createElement("style");
    style.textContent = STYLE;
    this.shadow.append(style);
    this.document.documentElement.append(host);
    this.host = host;
    const view = this.document.defaultView;
    if (view?.ResizeObserver) this.resizes = new view.ResizeObserver(this.reposition);
    this.scan();
    this.observer = new MutationObserver(() => this.rescanSoon());
    this.observer.observe(this.document, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ["type", "style", "class", "hidden", "disabled", "readonly"],
    });
    view?.addEventListener("scroll", this.reposition, { capture: true, passive: true });
    view?.addEventListener("resize", this.reposition, { passive: true });
    this.document.addEventListener("focusin", this.onFocusIn, true);
    this.document.addEventListener("pointerdown", this.onPointerDown, true);
    this.document.addEventListener("keydown", this.onKeyDown, true);
  }

  /** Removes the icons, for good: the user hid keefast on the site. */
  stop(): void {
    this.observer?.disconnect();
    this.resizes?.disconnect();
    clearTimeout(this.rescanTimer);
    const view = this.document.defaultView;
    if (this.frame !== undefined) view?.cancelAnimationFrame(this.frame);
    view?.removeEventListener("scroll", this.reposition, { capture: true });
    view?.removeEventListener("resize", this.reposition);
    this.document.removeEventListener("focusin", this.onFocusIn, true);
    this.document.removeEventListener("pointerdown", this.onPointerDown, true);
    this.document.removeEventListener("keydown", this.onKeyDown, true);
    this.host?.remove();
    this.host = undefined;
    this.shadow = undefined;
    this.icons.clear();
    this.tooltip = undefined;
  }

  /** Puts the icons on the fields there are now, and takes them off fields gone. */
  scan(): void {
    const shadow = this.shadow;
    if (!shadow) return;
    const fields = fieldsToMark(this.document, { paymentOnly: this.options.paymentOnly });
    const current = new Set(fields.map(({ input }) => input));
    for (const [input, { button }] of this.icons) {
      if (!current.has(input)) {
        button.remove();
        this.icons.delete(input);
        this.resizes?.unobserve(input);
        if (this.tooltip?.anchor === button) this.closeTooltip();
      }
    }
    for (const field of fields) {
      const icon = this.icons.get(field.input);
      if (icon && icon.field.kind === field.kind && icon.field.icon === field.icon) continue;
      icon?.button.remove();
      const button = this.iconFor(field);
      shadow.append(button);
      this.icons.set(field.input, { button, field });
      this.resizes?.observe(field.input);
    }
    this.place();
  }

  private iconFor(field: MarkedField): HTMLButtonElement {
    const button = this.document.createElement("button");
    button.type = "button";
    button.className = "icon";
    button.title = TITLES[field.kind];
    button.setAttribute("aria-label", TITLES[field.kind]);
    button.innerHTML = field.icon === "logo" ? LOGO_GLYPH : KEY_GLYPH;
    // The field keeps focus, as clicking inside it would.
    button.addEventListener("mousedown", (event) => event.preventDefault());
    button.addEventListener("click", (event) => {
      if (this.byUser(event)) void this.clicked(field, button);
    });
    return button;
  }

  private async clicked(field: MarkedField, button: HTMLButtonElement): Promise<void> {
    if (button.getAttribute("aria-busy") === "true") return;
    this.closeTooltip();
    // As clicking into the field would: the palette gives focus back to it once closed.
    field.input.focus({ preventScroll: true });
    button.setAttribute("aria-busy", "true");
    try {
      const answer = await this.handlers.click(field.kind);
      if (answer.kind === "unavailable") this.showTooltip(field, button, answer.status);
    } catch {
      // The extension was updated or reloaded meanwhile: it no longer answers this page.
      this.showTooltip(field, button, "failed");
    } finally {
      button.removeAttribute("aria-busy");
    }
  }

  private showTooltip(field: MarkedField, anchor: HTMLButtonElement, status: Unavailable): void {
    this.closeTooltip();
    const element = this.document.createElement("div");
    element.className = "tooltip";
    element.setAttribute("role", "status");
    const [title, detail] = explain(status);
    const titleText = this.document.createElement("p");
    titleText.className = "title";
    titleText.textContent = title;
    element.append(titleText);
    if (detail) {
      const detailText = this.document.createElement("p");
      detailText.className = "detail";
      detailText.textContent = detail;
      element.append(detailText);
    }
    if (status === "unpaired") {
      const connect = this.document.createElement("button");
      connect.type = "button";
      connect.textContent = "Connect";
      connect.addEventListener("click", async (event) => {
        if (!this.byUser(event)) return;
        connect.disabled = true;
        connect.textContent = "Approve in keefast…";
        const paired = await this.handlers
          .connect()
          .catch(() => ({ ok: false, reason: "failed" }) as const);
        if (paired.ok) {
          // Carry on with what the user clicked the icon for.
          this.closeTooltip();
          void this.clicked(field, anchor);
        } else {
          this.showTooltip(field, anchor, paired.reason);
        }
      });
      element.append(connect);
    }
    this.shadow?.append(element);
    this.tooltip = { element, anchor };
    this.placeTooltip();
  }

  private closeTooltip(): void {
    this.tooltip?.element.remove();
    this.tooltip = undefined;
  }

  /** Moves each icon to its field's right edge, and hides it while its field is hidden. */
  private place(): void {
    const view = this.document.defaultView;
    for (const [input, { button }] of this.icons) {
      const box = input.getBoundingClientRect();
      // A page that isn't laid out (as in tests) has no boxes to be in view.
      const inView =
        input.getClientRects().length === 0 ||
        (box.bottom > 0 &&
          box.right > 0 &&
          box.top < (view?.innerHeight ?? Infinity) &&
          box.left < (view?.innerWidth ?? Infinity));
      const visible = input.isConnected && shown(input) && inView;
      button.style.display = visible ? "" : "none";
      if (!visible) continue;
      button.style.left = `${box.right - ICON_SIZE - ICON_INSET}px`;
      button.style.top = `${box.top + (box.height - ICON_SIZE) / 2}px`;
    }
    this.placeTooltip();
  }

  private placeTooltip(): void {
    if (!this.tooltip) return;
    const { element, anchor } = this.tooltip;
    if (anchor.style.display === "none") return this.closeTooltip();
    const box = anchor.getBoundingClientRect();
    const width = this.document.defaultView?.innerWidth ?? Infinity;
    element.style.top = `${box.bottom + 8}px`;
    element.style.left = `${Math.max(8, Math.min(box.right - 260, width - 268))}px`;
  }

  private reposition = (): void => {
    const view = this.document.defaultView;
    if (!view || this.frame !== undefined) return;
    this.frame = view.requestAnimationFrame(() => {
      this.frame = undefined;
      this.place();
    });
  };

  private rescanSoon(): void {
    clearTimeout(this.rescanTimer);
    this.rescanTimer = setTimeout(() => this.scan(), RESCAN_DELAY_MS);
  }

  private onFocusIn = (event: Event): void => {
    if (event.target instanceof HTMLInputElement && !this.icons.has(event.target)) this.scan();
  };

  private onPointerDown = (event: Event): void => {
    // A click outside keefast's icons and tooltip closes it. Seen from the page, a closed
    // shadow root's events come from its host.
    if (this.tooltip && this.host && !event.composedPath().includes(this.host)) {
      this.closeTooltip();
    }
  };

  private onKeyDown = (event: KeyboardEvent): void => {
    if (event.key === "Escape") this.closeTooltip();
  };
}
