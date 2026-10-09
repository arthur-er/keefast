import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "wxt";

// The Companion extension, for Chromium-family browsers only (ADR 0003).
export default defineConfig({
  srcDir: "src",
  publicDir: "public",
  modules: ["@wxt-dev/module-react"],
  // Imports are written out, as everywhere else in keefast.
  imports: false,
  vite: () => ({ plugins: [tailwindcss()] }),
  zip: { artifactTemplate: "keefast-companion-{{version}}.zip" },
  manifest: {
    name: "keefast",
    description:
      "Fills sign-in forms with Entries from keefast, the KeePass password manager, when you ask.",
    minimum_chrome_version: "127",
    // Fixes the extension ID keefast's native host allows (crates/workspace/src/browser_hosts.rs).
    key: "MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAq7VveDeO1OCS7BsWExpznkfg/2h1h0UTo5iS9KQSR1bPVIFmDFh3926LGk/3j2DD6T9Jw2ObOlvzm0KXbzKlznMoGjnYImRI5L5N5sDEcBC9STBYsz5LtB8+9Y9TOPsJZRv1MQqigeXn6BPoJtdUZTOf/owsWqz+RZh23ODPy8GDEtWS8Adpo/R2b4uruwPbko8KUgP/jVBjqaJc+8d3nUHxFaqfCNpgle6MTflCTn6sFveH4P7+PgDyqkTD30143/hdCF52ZU1YauhirxK6+W5XxlK2zULLbwR0lpq7RZS0i6o2aA4mAaGrWWsSIPtdXBp3f2ndmbL2JqfoQBqFlQIDAQAB",
    permissions: ["activeTab", "nativeMessaging", "scripting", "storage"],
    // Granted only when the user turns on keefast in sign-in fields (ADR 0007).
    optional_host_permissions: ["<all_urls>"],
    action: {
      default_title: "keefast",
      default_icon: { 16: "icons/icon-16.png", 32: "icons/icon-32.png" },
    },
    icons: {
      16: "icons/icon-16.png",
      32: "icons/icon-32.png",
      48: "icons/icon-48.png",
      128: "icons/icon-128.png",
    },
    commands: {
      fill: {
        suggested_key: { default: "Alt+Shift+K" },
        description: "Fill this page's sign-in form",
      },
    },
    // The command palette is framed into pages; it answers only a palette keefast opened.
    web_accessible_resources: [{ resources: ["palette.html"], matches: ["<all_urls>"] }],
  },
});
