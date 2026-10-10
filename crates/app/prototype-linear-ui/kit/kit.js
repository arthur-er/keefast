/* PROTOTYPE — keefast shared kit: fake data, helpers, variant switcher. See ../DESIGN.md.
 *
 * Load order in a screen:
 *   <link rel="stylesheet" href="../kit/tokens.css">
 *   <link rel="stylesheet" href="../kit/components.css">
 *   <script src="https://unpkg.com/lucide@0.460.0/dist/umd/lucide.min.js"></script>
 *   <script src="../kit/kit.js"></script>
 *   <div id="app"></div>
 *   <script> KF.variants({ A: { name: "…", render(root) { … } }, B: … }) </script>
 */
(function () {
  const KF = (window.KF = {});

  /* ---------- Tiny helpers ---------- */
  KF.$ = (sel, root = document) => root.querySelector(sel);
  KF.$$ = (sel, root = document) => Array.from(root.querySelectorAll(sel));
  KF.esc = (s) => String(s ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));
  /** Lucide icon markup. */
  KF.icon = (name, cls = "") => `<i data-lucide="${name}" class="${cls}"></i>`;
  KF.kbd = (...keys) => `<span class="kbds">${keys.map((k) => `<kbd>${k}</kbd>`).join("")}</span>`;
  KF.mask = (n = 12) => "•".repeat(n);

  // Turn every new [data-lucide] into an SVG automatically.
  function hydrateIcons() { if (window.lucide) window.lucide.createIcons({ attrs: { "stroke-width": 1.75 } }); }
  let pending = false;
  new MutationObserver(() => {
    if (pending) return;
    pending = true;
    requestAnimationFrame(() => { pending = false; hydrateIcons(); });
  }).observe(document.documentElement, { childList: true, subtree: true });
  KF.hydrateIcons = hydrateIcons;

  /* ---------- Entry avatar (favicon stand-in) ---------- */
  const SITE_COLORS = {
    github: "#24292f", google: "#4285f4", gmail: "#ea4335", aws: "#ff9900", stripe: "#635bff",
    figma: "#a259ff", linear: "#5e6ad2", slack: "#4a154b", notion: "#2f2f2f", dropbox: "#0061fe",
    netflix: "#e50914", spotify: "#1db954", apple: "#555555", vercel: "#000000", cloudflare: "#f38020",
    digitalocean: "#0080ff", postgres: "#336791", ssh: "#3a3f47", bank: "#0f766e", wifi: "#7c3aed",
    amazon: "#232f3e", twitter: "#1d9bf0", proton: "#6d4aff", fastmail: "#0067b9", tailscale: "#242424",
    hetzner: "#d50c2d", docker: "#2496ed", npm: "#cb3837", openai: "#10a37f", gitlab: "#fc6d26",
  };
  KF.avatar = (entry, size = 28) => {
    const bg = SITE_COLORS[entry.brand] || "#3a3d45";
    const letter = (entry.title || "?").trim()[0].toUpperCase();
    const fs = Math.round(size * 0.46);
    const r = Math.max(5, Math.round(size * 0.24));
    return `<span class="avatar" style="width:${size}px;height:${size}px;border-radius:${r}px;background:${bg};font-size:${fs}px">${KF.esc(letter)}</span>`;
  };
  KF.colorVar = (c) => (c ? `var(--c-${c})` : "transparent");

  /* ---------- Fake Vaults (shared story for every screen) ---------- */
  const E = (o) => Object.assign({
    username: "", password: "", url: "", notes: "", tags: [], color: null, totp: null,
    attachments: [], fields: [], expires: null, history: 0, group: "root",
    created: "2024-03-02", modified: "2026-09-28", access: [],
  }, o);

  KF.data = {
    vaults: [
      {
        id: "personal", name: "Personal", file: "~/Vaults/Personal.kdbx", storage: "local",
        status: "saved", locked: false,
        groups: [
          { id: "root", name: "Personal", parent: null, icon: "vault" },
          { id: "email", name: "Email", parent: "root", icon: "mail" },
          { id: "finance", name: "Finance", parent: "root", icon: "landmark" },
          { id: "social", name: "Social", parent: "root", icon: "users" },
          { id: "home", name: "Home", parent: "root", icon: "house" },
          { id: "browser", name: "Browser logins", parent: "root", icon: "globe", browserGroup: true },
          { id: "bin", name: "Recycle Bin", parent: "root", icon: "trash-2", recycle: true },
        ],
        entries: [
          E({ id: "e1", title: "GitHub", brand: "github", username: "arthur-er", password: "x7#Lq9!vR2mZ@pW4", url: "https://github.com", group: "root", tags: ["dev", "2fa"], color: "purple", totp: { secret: "JBSWY3DPEHPK3PXP", code: "482913", period: 30, left: 18 }, notes: "Recovery codes in attachment.", attachments: [{ name: "github-recovery-codes.txt", size: "1.2 KB" }], fields: [{ name: "PAT (ci)", value: "ghp_4fT9…redacted", protected: true }], history: 7, modified: "2026-10-07", access: [{ site: "github.com", decision: "allow" }] }),
          E({ id: "e2", title: "Proton Mail", brand: "proton", username: "arthur-er@protonmail.com", password: "correct-horse-battery-staple-91", url: "https://account.proton.me", group: "email", tags: ["email", "2fa"], color: "blue", totp: { code: "903 117", period: 30, left: 9 }, history: 3, modified: "2026-09-30" }),
          E({ id: "e3", title: "Google", brand: "google", username: "arthur.rezende@gmail.com", password: "Gq!29vmL0pzX#", url: "https://accounts.google.com", group: "email", tags: ["email"], history: 12, modified: "2026-08-14" }),
          E({ id: "e4", title: "Fastmail", brand: "fastmail", username: "a@rezende.dev", password: "uH3$kq8Zx1!b", url: "https://app.fastmail.com", group: "email", tags: ["email"], modified: "2026-05-02" }),
          E({ id: "e5", title: "Nubank", brand: "bank", username: "123.456.789-00", password: "8271", url: "https://nubank.com.br", group: "finance", tags: ["bank"], color: "green", notes: "Card PIN is the password. Support: 0800 591 2117", fields: [{ name: "Account", value: "0001 / 9876543-2", protected: false }], history: 2, modified: "2026-07-21" }),
          E({ id: "e6", title: "Stripe", brand: "stripe", username: "billing@rezende.dev", password: "pK8!mZ2#qR7vLx0w", url: "https://dashboard.stripe.com", group: "finance", tags: ["work", "2fa"], totp: { code: "551 204", period: 30, left: 24 }, modified: "2026-09-11" }),
          E({ id: "e7", title: "Amazon", brand: "amazon", username: "arthur.rezende@gmail.com", password: "Am4z0n!Shop#22", url: "https://amazon.com.br", group: "root", tags: ["shopping"], expires: "2026-09-01", modified: "2025-11-19" }),
          E({ id: "e8", title: "Netflix", brand: "netflix", username: "family@rezende.dev", password: "couch-popcorn-42", url: "https://netflix.com", group: "home", tags: ["shared"], color: "red", modified: "2026-01-05" }),
          E({ id: "e9", title: "Spotify", brand: "spotify", username: "arthur-er", password: "Sp0t!fyBeats", url: "https://open.spotify.com", group: "home", tags: ["shared"], modified: "2025-12-12" }),
          E({ id: "e10", title: "Home Wi‑Fi", brand: "wifi", username: "rezende-5g", password: "lantern-orbit-maple-07", url: "", group: "home", tags: ["home"], color: "yellow", notes: "Router admin at 192.168.0.1 (admin / see custom field).", fields: [{ name: "Router admin", value: "Rtr!adm1n", protected: true }], modified: "2026-02-28" }),
          E({ id: "e11", title: "X / Twitter", brand: "twitter", username: "@arthur_er", password: "Tw33t!ng#2026", url: "https://x.com", group: "social", tags: ["social"], modified: "2026-04-10" }),
          E({ id: "e12", title: "Apple ID", brand: "apple", username: "arthur.rezende@icloud.com", password: "Appl3!D#seed", url: "https://appleid.apple.com", group: "root", tags: ["2fa"], color: "orange", totp: { code: "118 640", period: 30, left: 4 }, modified: "2026-06-03" }),
          E({ id: "e13", title: "Figma", brand: "figma", username: "arthur@rezende.dev", password: "F1gm@Frames!", url: "https://figma.com", group: "browser", tags: ["design"], modified: "2026-10-02" }),
          E({ id: "e14", title: "Old Yahoo", brand: "", username: "arthur_r_1999", password: "password123", url: "https://yahoo.com", group: "bin", modified: "2023-01-01" }),
        ],
      },
      {
        id: "work", name: "Work", file: "Dropbox: /Apps/keefast/Work.kdbx", storage: "dropbox",
        status: "synced", locked: false,
        groups: [
          { id: "root", name: "Work", parent: null, icon: "briefcase" },
          { id: "infra", name: "Infrastructure", parent: "root", icon: "server" },
          { id: "servers", name: "Servers", parent: "infra", icon: "hard-drive" },
          { id: "saas", name: "SaaS", parent: "root", icon: "cloud" },
          { id: "bin", name: "Recycle Bin", parent: "root", icon: "trash-2", recycle: true },
        ],
        entries: [
          E({ id: "w1", title: "AWS root", brand: "aws", username: "root@rezende.dev", password: "Aws!R00t#Kq7zPm", url: "https://console.aws.amazon.com", group: "infra", tags: ["prod", "2fa"], color: "red", totp: { code: "774 029", period: 30, left: 13 }, fields: [{ name: "Account ID", value: "4821-9930-1174", protected: false }], history: 4, modified: "2026-10-08" }),
          E({ id: "w2", title: "Cloudflare", brand: "cloudflare", username: "ops@rezende.dev", password: "Cl0udfl@re!dns", url: "https://dash.cloudflare.com", group: "infra", tags: ["prod"], modified: "2026-09-02" }),
          E({ id: "w3", title: "prod-db-01", brand: "postgres", username: "postgres", password: "pg_Pr0d!7f2Kx9", url: "postgres://10.0.4.12:5432", group: "servers", tags: ["prod", "db"], color: "red", notes: "Primary. Replica is prod-db-02.", modified: "2026-09-29" }),
          E({ id: "w4", title: "bastion", brand: "ssh", username: "deploy", password: "", url: "ssh://bastion.rezende.dev", group: "servers", tags: ["ssh"], attachments: [{ name: "id_ed25519", size: "411 B" }, { name: "id_ed25519.pub", size: "98 B" }], modified: "2026-08-30" }),
          E({ id: "w5", title: "Linear", brand: "linear", username: "arthur@rezende.dev", password: "L1near!Issues", url: "https://linear.app", group: "saas", tags: ["work"], modified: "2026-09-15" }),
          E({ id: "w6", title: "Slack", brand: "slack", username: "arthur@rezende.dev", password: "Sl@ck!Chan#9", url: "https://rezende.slack.com", group: "saas", tags: ["work"], modified: "2026-07-07" }),
          E({ id: "w7", title: "Vercel", brand: "vercel", username: "arthur-er", password: "V3rc3l!Edge", url: "https://vercel.com", group: "saas", tags: ["work", "2fa"], totp: { code: "330 981", period: 30, left: 27 }, modified: "2026-09-20" }),
          E({ id: "w8", title: "Hetzner", brand: "hetzner", username: "K0123456789", password: "Htz!B0x#2025", url: "https://console.hetzner.cloud", group: "infra", tags: ["prod"], expires: "2026-10-20", modified: "2026-04-22" }),
        ],
      },
      {
        id: "family", name: "Family", file: "~/Vaults/Family.kdbx", storage: "local", status: "locked", locked: true,
        groups: [], entries: [],
      },
    ],
    tags: ["2fa", "dev", "email", "bank", "work", "prod", "shared", "home", "social", "db", "ssh", "shopping", "design"],
    colors: ["red", "orange", "yellow", "green", "blue", "purple"],
    recents: [
      { name: "Personal", path: "~/Vaults/Personal.kdbx", storage: "local", opened: "Today" },
      { name: "Work", path: "Dropbox: /Apps/keefast/Work.kdbx", storage: "dropbox", opened: "Today" },
      { name: "Family", path: "~/Vaults/Family.kdbx", storage: "local", opened: "Yesterday" },
      { name: "Old-2019", path: "~/Backups/Old-2019.kdbx", storage: "local", opened: "3 months ago" },
    ],
    pairings: [
      { name: "Chrome on my laptop", created: "2026-08-02", lastUsed: "2 minutes ago" },
      { name: "Brave (work profile)", created: "2026-09-14", lastUsed: "Yesterday" },
    ],
  };
  KF.vault = (id = "personal") => KF.data.vaults.find((v) => v.id === id);
  KF.entry = (id) => KF.data.vaults.flatMap((v) => v.entries).find((e) => e.id === id);
  KF.groupPath = (vault, gid) => {
    const out = [];
    let g = vault.groups.find((x) => x.id === gid);
    while (g) { out.unshift(g); g = vault.groups.find((x) => x.id === g.parent); }
    return out;
  };
  KF.host = (url) => { try { return new URL(url).host; } catch { return url; } };

  /* ---------- Toast + copy ---------- */
  KF.toast = (html, { seconds = 0, icon = "check" } = {}) => {
    let host = document.querySelector(".toast-host");
    if (!host) { host = document.createElement("div"); host.className = "toast-host"; document.getElementById("app").appendChild(host); }
    host.innerHTML = "";
    const t = document.createElement("div");
    t.className = "toast";
    t.innerHTML = `${KF.icon(icon)}<span>${html}</span>${seconds ? `<span class="subtle mono" data-left>${seconds}s</span><div class="toast-bar" style="animation-duration:${seconds}s"></div>` : ""}`;
    host.appendChild(t);
    clearTimeout(KF._toastT);
    KF._toastT = setTimeout(() => t.remove(), seconds ? Math.min(seconds, 4) * 1000 : 2400);
    return t;
  };
  KF.copy = (what, el) => {
    KF.toast(`${KF.esc(what)} copied <span class="subtle">· clipboard clears in 30s</span>`, { seconds: 30, icon: "clipboard-check" });
    if (el) { el.classList.add("is-copied"); setTimeout(() => el.classList.remove("is-copied"), 1200); }
  };

  /* ---------- Theme ---------- */
  KF.setTheme = (t) => {
    if (t === "light") document.documentElement.dataset.theme = "light";
    else delete document.documentElement.dataset.theme;
    const u = new URL(location.href);
    if (t === "light") u.searchParams.set("theme", "light"); else u.searchParams.delete("theme");
    history.replaceState(null, "", u);
    const btn = document.querySelector(".proto-bar [data-theme]");
    if (btn) btn.innerHTML = KF.icon(t === "light" ? "moon" : "sun", "ico-sm");
  };
  if (new URL(location.href).searchParams.get("theme") === "light") document.documentElement.dataset.theme = "light";

  /* ---------- Variant switcher (prototype chrome, not design) ---------- */
  KF.variants = (defs) => {
    const keys = Object.keys(defs);
    const root = document.getElementById("app");
    const bar = document.createElement("div");
    bar.className = "proto-bar";
    document.body.appendChild(bar);
    if (new URL(location.href).searchParams.get("shot") === "1") bar.style.display = "none";

    function current() {
      const v = new URL(location.href).searchParams.get("variant");
      return keys.includes(v) ? v : keys[0];
    }
    function go(key) {
      const u = new URL(location.href);
      u.searchParams.set("variant", key);
      history.replaceState(null, "", u);
      render();
    }
    function step(d) { const i = keys.indexOf(current()); go(keys[(i + d + keys.length) % keys.length]); }
    function render() {
      const key = current();
      root.innerHTML = "";
      root.className = "";
      defs[key].render(root);
      document.title = `${key} · ${defs[key].name} — keefast prototype`;
      bar.innerHTML = `
        <a href="../index.html" title="All screens">${KF.icon("layout-grid", "ico-sm")}</a>
        <span class="sep"></span>
        <button data-d="-1" title="Previous variant (←)">←</button>
        <span class="proto-label"><b>${key}</b> <small>${KF.esc(defs[key].name)}</small> <small>· ${keys.indexOf(key) + 1}/${keys.length}</small></span>
        <button data-d="1" title="Next variant (→)">→</button>
        <span class="sep"></span>
        <button data-theme title="Toggle theme (T)">${KF.icon(document.documentElement.dataset.theme === "light" ? "moon" : "sun", "ico-sm")}</button>`;
      bar.querySelectorAll("[data-d]").forEach((b) => (b.onclick = () => step(+b.dataset.d)));
      bar.querySelector("[data-theme]").onclick = toggleTheme;
      console.log("[prototype] variant", key, defs[key].name);
    }
    function toggleTheme() { KF.setTheme(document.documentElement.dataset.theme === "light" ? "dark" : "light"); render(); }
    window.addEventListener("keydown", (e) => {
      const t = e.target;
      if (t.closest && t.closest("input, textarea, [contenteditable], select")) return;
      if (e.ctrlKey || e.metaKey || e.altKey) return;
      if (e.key === "ArrowLeft") { e.preventDefault(); step(-1); }
      else if (e.key === "ArrowRight") { e.preventDefault(); step(1); }
      else if (e.key === "t" || e.key === "T") toggleTheme();
    });
    render();
  };
})();
