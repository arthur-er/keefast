# Browser extension is a secret-less companion to the desktop app

The browser extension never opens or decrypts Vault files itself. It asks the running, unlocked desktop app for credentials over native messaging. We rejected a standalone extension (Rust core compiled to WASM) so there is a single trust boundary and Vault contents never live in browser storage. The trade-off: autofill only works while the desktop app is running and unlocked. If the app is locked or not running, the extension fails and tells the user to unlock keefast. It does not launch the app or offer to unlock it.
