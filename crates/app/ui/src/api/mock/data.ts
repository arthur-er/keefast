// The mock backend's fake Vaults: the shared story of the Linear-style prototype
// (feat/linear-style-ui-prototypes, crates/app/prototype-linear-ui/kit/kit.js, `KF.data`), so
// screens built against the mock can be compared with the prototype's screenshots.

import type { Color, Location, SyncState } from "@/bindings";

export interface MockTotp {
  secret: string;
  algorithm: "sha1" | "sha256" | "sha512";
  digits: number;
  period: number;
}

export interface MockAttachment {
  name: string;
  /** Text contents, or a `data:` URL for an image, or bytes it can't show (a size only). */
  text?: string;
  image?: string;
  size: number;
}

export interface MockField {
  name: string;
  value: string;
  protected: boolean;
}

export interface MockVersion {
  modified: number;
  title: string;
  username: string;
  password: string;
  url: string;
  notes: string;
  tags: string[];
  color: Color | null;
  fields: MockField[];
  totp: MockTotp | null;
  attachments: string[];
}

export interface MockEntry {
  id: string;
  title: string;
  /** The brand the prototype tinted the Entry's avatar with. */
  brand: string;
  username: string;
  password: string;
  url: string;
  notes: string;
  tags: string[];
  color: Color | null;
  icon: { kind: "standard"; number: number } | { kind: "custom"; id: string };
  totp: MockTotp | null;
  attachments: MockAttachment[];
  fields: MockField[];
  expires: number | null;
  created: number;
  modified: number;
  group: string;
  history: MockVersion[];
  access: { host: string; allowed: boolean }[];
}

export interface MockGroup {
  id: string;
  name: string;
  parent: string | null;
}

export interface MockVault {
  id: number;
  name: string;
  location: Location;
  locked: boolean;
  groups: MockGroup[];
  entries: MockEntry[];
  recycleBin: string | null;
  browserGroup: string | null;
  unsaved: boolean;
  saving: boolean;
  saveFailed: boolean;
  sync: SyncState | null;
  pairings: string[];
  policy: "ask" | "alwaysAllow" | "alwaysDeny";
  customIcons: { id: string; png: string }[];
}

const day = (date: string) => Date.parse(`${date}T12:00:00Z`) / 1000;

/** The prototype's site colours, for the Entries' stand-in favicons. */
export const BRAND_COLORS: Record<string, string> = {
  github: "#24292f", google: "#4285f4", gmail: "#ea4335", aws: "#ff9900", stripe: "#635bff",
  figma: "#a259ff", linear: "#5e6ad2", slack: "#4a154b", notion: "#2f2f2f", dropbox: "#0061fe",
  netflix: "#e50914", spotify: "#1db954", apple: "#555555", vercel: "#000000", cloudflare: "#f38020",
  digitalocean: "#0080ff", postgres: "#336791", ssh: "#3a3f47", bank: "#0f766e", wifi: "#7c3aed",
  amazon: "#232f3e", twitter: "#1d9bf0", proton: "#6d4aff", fastmail: "#0067b9", tailscale: "#242424",
  hetzner: "#d50c2d", docker: "#2496ed", npm: "#cb3837", openai: "#10a37f", gitlab: "#fc6d26",
};

const TOTP = (secret: string): MockTotp => ({ secret, algorithm: "sha1", digits: 6, period: 30 });

interface Seed {
  id: string;
  title: string;
  brand?: string;
  username?: string;
  password?: string;
  url?: string;
  notes?: string;
  tags?: string[];
  color?: Color;
  totp?: string;
  attachments?: MockAttachment[];
  fields?: MockField[];
  expires?: string;
  history?: number;
  modified?: string;
  created?: string;
  group?: string;
  access?: { host: string; allowed: boolean }[];
}

/** An Entry, with `history` earlier versions made up from it. */
function entry(seed: Seed): MockEntry {
  const modified = day(seed.modified ?? "2026-09-28");
  const made: MockEntry = {
    id: seed.id,
    title: seed.title,
    brand: seed.brand ?? "",
    username: seed.username ?? "",
    password: seed.password ?? "",
    url: seed.url ?? "",
    notes: seed.notes ?? "",
    tags: seed.tags ?? [],
    color: seed.color ?? null,
    icon: { kind: "standard", number: seed.url ? 1 : 0 },
    totp: seed.totp ? TOTP(seed.totp) : null,
    attachments: seed.attachments ?? [],
    fields: seed.fields ?? [],
    expires: seed.expires ? day(seed.expires) : null,
    created: day(seed.created ?? "2024-03-02"),
    modified,
    group: seed.group ?? "root",
    history: [],
    access: seed.access ?? [],
  };
  for (let index = 0; index < (seed.history ?? 0); index++) {
    const back = (seed.history! - index) * 23 * 86400;
    made.history.push({
      modified: modified - back,
      title: made.title,
      username: index === 0 ? made.username.replace(/@.*/, "@old-mail.com") : made.username,
      password: index % 2 === 0 ? `${made.password.slice(0, 6)}-old${index}` : made.password,
      url: made.url,
      notes: index === 0 ? "" : made.notes,
      tags: made.tags.slice(0, Math.max(0, made.tags.length - 1)),
      color: index === 0 ? null : made.color,
      fields: made.fields,
      totp: index < 2 ? null : made.totp,
      attachments: [],
    });
  }
  return made;
}

const RECOVERY_CODES = [
  "GitHub recovery codes",
  "",
  "a1b2c-3d4e5  f6g7h-8i9j0  k1l2m-3n4o5  p6q7r-8s9t0",
  "u1v2w-3x4y5  z6a7b-8c9d0  e1f2g-3h4i5  j6k7l-8m9n0",
].join("\n");

const PUBLIC_KEY = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl deploy@bastion";

export function fakeVaults(): MockVault[] {
  return [
    {
      id: 0,
      name: "Personal",
      location: { provider: "local", path: "~/Vaults/Personal.kdbx" },
      locked: false,
      groups: [
        { id: "root", name: "Personal", parent: null },
        { id: "email", name: "Email", parent: "root" },
        { id: "finance", name: "Finance", parent: "root" },
        { id: "social", name: "Social", parent: "root" },
        { id: "home", name: "Home", parent: "root" },
        { id: "browser", name: "Browser logins", parent: "root" },
        { id: "bin", name: "Recycle Bin", parent: "root" },
      ],
      recycleBin: "bin",
      browserGroup: "browser",
      unsaved: false,
      saving: false,
      saveFailed: false,
      sync: null,
      pairings: ["Chrome on my laptop", "Brave (work profile)"],
      policy: "ask",
      customIcons: [],
      entries: [
        entry({ id: "e1", title: "GitHub", brand: "github", username: "arthur-er", password: "x7#Lq9!vR2mZ@pW4", url: "https://github.com", tags: ["dev", "2fa"], color: "violet", totp: "JBSWY3DPEHPK3PXP", notes: "Recovery codes in attachment.", attachments: [{ name: "github-recovery-codes.txt", text: RECOVERY_CODES, size: 1229 }], fields: [{ name: "PAT (ci)", value: "ghp_4fT9xQ2mZr8LbN0vKc7WdY1sHe3JpA6uGi5o", protected: true }], history: 7, modified: "2026-10-07", access: [{ host: "github.com", allowed: true }] }),
        entry({ id: "e2", title: "Proton Mail", brand: "proton", username: "arthur-er@protonmail.com", password: "correct-horse-battery-staple-91", url: "https://account.proton.me", group: "email", tags: ["email", "2fa"], color: "blue", totp: "KRSXG5CTMVRXEZLU", history: 3, modified: "2026-09-30" }),
        entry({ id: "e3", title: "Google", brand: "google", username: "arthur.rezende@gmail.com", password: "Gq!29vmL0pzX#", url: "https://accounts.google.com", group: "email", tags: ["email"], history: 12, modified: "2026-08-14" }),
        entry({ id: "e4", title: "Fastmail", brand: "fastmail", username: "a@rezende.dev", password: "uH3$kq8Zx1!b", url: "https://app.fastmail.com", group: "email", tags: ["email"], modified: "2026-05-02" }),
        entry({ id: "e5", title: "Nubank", brand: "bank", username: "123.456.789-00", password: "8271", url: "https://nubank.com.br", group: "finance", tags: ["bank"], color: "green", notes: "Card PIN is the password. Support: 0800 591 2117", fields: [{ name: "Account", value: "0001 / 9876543-2", protected: false }], history: 2, modified: "2026-07-21" }),
        entry({ id: "e6", title: "Stripe", brand: "stripe", username: "billing@rezende.dev", password: "pK8!mZ2#qR7vLx0w", url: "https://dashboard.stripe.com", group: "finance", tags: ["work", "2fa"], totp: "MFRGGZDFMZTWQ2LK", modified: "2026-09-11" }),
        entry({ id: "e7", title: "Amazon", brand: "amazon", username: "arthur.rezende@gmail.com", password: "Am4z0n!Shop#22", url: "https://amazon.com.br", tags: ["shopping"], expires: "2026-09-01", modified: "2025-11-19" }),
        entry({ id: "e8", title: "Netflix", brand: "netflix", username: "family@rezende.dev", password: "couch-popcorn-42", url: "https://netflix.com", group: "home", tags: ["shared"], color: "red", modified: "2026-01-05" }),
        entry({ id: "e9", title: "Spotify", brand: "spotify", username: "arthur-er", password: "Sp0t!fyBeats", url: "https://open.spotify.com", group: "home", tags: ["shared"], modified: "2025-12-12" }),
        entry({ id: "e10", title: "Home Wi‑Fi", brand: "wifi", username: "rezende-5g", password: "lantern-orbit-maple-07", group: "home", tags: ["home"], color: "yellow", notes: "Router admin at 192.168.0.1 (admin / see custom field).", fields: [{ name: "Router admin", value: "Rtr!adm1n", protected: true }], modified: "2026-02-28" }),
        entry({ id: "e11", title: "X / Twitter", brand: "twitter", username: "@arthur_er", password: "Tw33t!ng#2026", url: "https://x.com", group: "social", tags: ["social"], modified: "2026-04-10" }),
        entry({ id: "e12", title: "Apple ID", brand: "apple", username: "arthur.rezende@icloud.com", password: "Appl3!D#seed", url: "https://appleid.apple.com", tags: ["2fa"], color: "orange", totp: "GEZDGNBVGY3TQOJQ", modified: "2026-06-03" }),
        entry({ id: "e13", title: "Figma", brand: "figma", username: "arthur@rezende.dev", password: "F1gm@Frames!", url: "https://figma.com", group: "browser", tags: ["design"], modified: "2026-10-02" }),
        entry({ id: "e14", title: "Old Yahoo", username: "arthur_r_1999", password: "password123", url: "https://yahoo.com", group: "bin", modified: "2023-01-01" }),
      ],
    },
    {
      id: 1,
      name: "Work",
      location: { provider: "dropbox", path: "/Apps/keefast/Work.kdbx" },
      locked: false,
      groups: [
        { id: "root", name: "Work", parent: null },
        { id: "infra", name: "Infrastructure", parent: "root" },
        { id: "servers", name: "Servers", parent: "infra" },
        { id: "saas", name: "SaaS", parent: "root" },
        { id: "bin", name: "Recycle Bin", parent: "root" },
      ],
      recycleBin: "bin",
      browserGroup: null,
      unsaved: false,
      saving: false,
      saveFailed: false,
      sync: "synced",
      pairings: ["Chrome on my laptop"],
      policy: "ask",
      customIcons: [],
      entries: [
        entry({ id: "w1", title: "AWS root", brand: "aws", username: "root@rezende.dev", password: "Aws!R00t#Kq7zPm", url: "https://console.aws.amazon.com", group: "infra", tags: ["prod", "2fa"], color: "red", totp: "NBSWY3DPO5XXE3DE", fields: [{ name: "Account ID", value: "4821-9930-1174", protected: false }], history: 4, modified: "2026-10-08" }),
        entry({ id: "w2", title: "Cloudflare", brand: "cloudflare", username: "ops@rezende.dev", password: "Cl0udfl@re!dns", url: "https://dash.cloudflare.com", group: "infra", tags: ["prod"], modified: "2026-09-02" }),
        entry({ id: "w3", title: "prod-db-01", brand: "postgres", username: "postgres", password: "pg_Pr0d!7f2Kx9", url: "postgres://10.0.4.12:5432", group: "servers", tags: ["prod", "db"], color: "red", notes: "Primary. Replica is prod-db-02.", modified: "2026-09-29" }),
        entry({ id: "w4", title: "bastion", brand: "ssh", username: "deploy", url: "ssh://bastion.rezende.dev", group: "servers", tags: ["ssh"], attachments: [{ name: "id_ed25519", size: 411 }, { name: "id_ed25519.pub", text: PUBLIC_KEY, size: 98 }], modified: "2026-08-30" }),
        entry({ id: "w5", title: "Linear", brand: "linear", username: "arthur@rezende.dev", password: "L1near!Issues", url: "https://linear.app", group: "saas", tags: ["work"], modified: "2026-09-15" }),
        entry({ id: "w6", title: "Slack", brand: "slack", username: "arthur@rezende.dev", password: "Sl@ck!Chan#9", url: "https://rezende.slack.com", group: "saas", tags: ["work"], modified: "2026-07-07" }),
        entry({ id: "w7", title: "Vercel", brand: "vercel", username: "arthur-er", password: "V3rc3l!Edge", url: "https://vercel.com", group: "saas", tags: ["work", "2fa"], totp: "ONSWG4TFOQYTEMZU", modified: "2026-09-20" }),
        entry({ id: "w8", title: "Hetzner", brand: "hetzner", username: "K0123456789", password: "Htz!B0x#2025", url: "https://console.hetzner.cloud", group: "infra", tags: ["prod"], expires: "2026-10-20", modified: "2026-04-22" }),
      ],
    },
    {
      id: 2,
      name: "Family",
      location: { provider: "local", path: "~/Vaults/Family.kdbx" },
      locked: true,
      groups: [
        { id: "root", name: "Family", parent: null },
        { id: "bin", name: "Recycle Bin", parent: "root" },
      ],
      recycleBin: "bin",
      browserGroup: null,
      unsaved: false,
      saving: false,
      saveFailed: false,
      sync: null,
      pairings: [],
      policy: "ask",
      customIcons: [],
      entries: [
        entry({ id: "f1", title: "School portal", brand: "", username: "parent@rezende.dev", password: "Sch00l!Days", url: "https://portal.school.example", modified: "2026-03-14" }),
        entry({ id: "f2", title: "Streaming (TV)", brand: "netflix", username: "family@rezende.dev", password: "tv-remote-1234", url: "https://netflix.com", tags: ["shared"], modified: "2026-02-01" }),
      ],
    },
  ];
}

/** Recent Vault files that aren't open, as the prototype's Open a Vault screen lists them. */
export const fakeRecents: Location[] = [{ provider: "local", path: "~/Backups/Old-2019.kdbx" }];
