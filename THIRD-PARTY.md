# Third-party software

Corepass is an independent controller and UI. It does not modify the VPN engine.

- **sing-box v1.14.3**: GPL-3.0-or-later; [license](https://github.com/SagerNet/sing-box/blob/v1.14.3/LICENSE), [complete upstream source and build instructions](https://github.com/SagerNet/sing-box/tree/v1.14.3), [source archive](https://github.com/SagerNet/sing-box/archive/refs/tags/v1.14.3.tar.gz), [official binaries](https://github.com/SagerNet/sing-box/releases/tag/v1.14.3). Redistributed unmodified as a separate executable, alongside its upstream license. sing-box's dependencies retain their upstream licenses.
- **Cronet / Chromium**: included in the upstream sing-box Windows release, Chromium BSD-style and third-party licenses; [source](https://github.com/SagerNet/cronet-go), [Chromium license](https://chromium.googlesource.com/chromium/src/+/main/LICENSE).
- **Wintun**: [official distribution and source](https://www.wintun.net/), upstream license in the engine directory. Used by sing-box for Windows TUN.
- **Tauri 2 / Wry / Tao**: MIT or Apache-2.0; https://github.com/tauri-apps/tauri
- **React**: MIT; https://github.com/facebook/react
- **Lucide icons**: ISC; https://github.com/lucide-icons/lucide
- **Rust dependencies**: versions recorded in Cargo.lock; their upstream license notices apply.

Router engine images contain the same unmodified upstream Linux-musl executable, stored in a read-only compressed SquashFS image. Corepass scripts, UI and original source are MIT; the engine remains under its own license.
