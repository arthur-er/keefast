use tauri_build::{Attributes, WindowsAttributes};

fn main() {
    // The C runtime is linked statically by `crt-static` in .cargo/config.toml. Tauri's own way,
    // an empty msvcrt.lib on the library path, reaches the other crates' doctests and breaks them.
    let attributes =
        Attributes::new().windows_attributes(WindowsAttributes::new().static_vc_runtime(false));
    if let Err(error) = tauri_build::try_build(attributes) {
        panic!("{error:#}");
    }
}
