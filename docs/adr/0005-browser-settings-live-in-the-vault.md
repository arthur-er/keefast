# Browser-integration settings live inside the Vault file

Pairings, the Default access policy, remembered Access decisions and the Browser group are stored in the Vault file's custom data, using KeePassXC-compatible keys where they exist, not in keefast's app config. This lets them travel with the Vault across machines and stay compatible with KeePassXC. The app config (TOML) holds only machine-local preferences, such as recent Vault files, theme, lock timings and the browser integration toggle. It never holds secrets: Dropbox tokens go in the OS secret store.
