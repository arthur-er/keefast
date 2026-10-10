The app's icons, made from `assets/icon.svg` by Tauri's CLI. After changing the SVG, make them again
from the repo root, and keep only the files already here:

```sh
cargo tauri icon assets/icon.svg -o crates/app/icons
```

`128x128@2x.png` comes first in `tauri.conf.json`'s list, as Tauri makes the first PNG the window's
icon on Linux. With the 512 px `icon.png` there, the window had no icon under X11.
