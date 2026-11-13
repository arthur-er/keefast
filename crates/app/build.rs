use tauri_build::{Attributes, WindowsAttributes};

fn main() {
    // The release build links the C runtime statically, so keefast.exe needs no Visual C++
    // redistributable. Tauri does it by putting an empty msvcrt.lib on the library path, which
    // reaches the other crates' doctests too and breaks them, so only release builds get it.
    let release = std::env::var("PROFILE").as_deref() == Ok("release");
    let attributes =
        Attributes::new().windows_attributes(WindowsAttributes::new().static_vc_runtime(release));
    if let Err(error) = tauri_build::try_build(attributes) {
        panic!("{error:#}");
    }
}
