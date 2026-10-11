# keefast-plugin-sdk

Write a [keefast](https://github.com/arthur-er/keefast) Plugin in Rust: the host API's bindings,
the `Plugin` trait for the `format`, `validate` and `derive` hooks, and typed access to a facet's
Owned fields.

```rust
use keefast_plugin_sdk::{Derived, Fields, Plugin};

struct Iban;

impl Plugin for Iban {
    fn derive(_facet: &str, fields: &Fields) -> Derived {
        Derived::new().label(fields.text("iban").map(|iban| format!("IBAN {}", &iban[..4])))
    }
}

keefast_plugin_sdk::export!(Iban);
```

Not to be confused with `keefast-plugins`, keefast's own Plugin host, which this crate doesn't
depend on. Build and pack a Plugin with the `keefast-plugin` command (`keefast-plugin-cli`).

**The guide is [docs/plugins/README.md](../../docs/plugins/README.md)**: the manifest, the hooks,
the contribution kinds, the Plugin capabilities, the sandbox, and packaging and signing. Start from
the template, [`plugins/template/`](../../plugins/template).
