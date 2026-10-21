# Third-party software

Corepass is an independent controller and UI. It does not modify the VPN engine.

- **sing-box v1.14.3**: GPL-3.0-or-later; [license](https://github.com/SagerNet/sing-box/blob/v1.14.3/LICENSE), [complete upstream source and build instructions](https://github.com/SagerNet/sing-box/tree/v1.14.3), [source archive](https://github.com/SagerNet/sing-box/archive/refs/tags/v1.14.3.tar.gz), [official binaries](https://github.com/SagerNet/sing-box/releases/tag/v1.14.3). Redistributed unmodified as a separate executable, alongside its upstream license. sing-box's dependencies retain their upstream licenses.
- **Cronet / Chromium**: included in the upstream sing-box Windows release, Chromium BSD-style and third-party licenses; [source](https://github.com/SagerNet/cronet-go), [Chromium license](https://chromium.googlesource.com/chromium/src/+/main/LICENSE).
- **Wintun 0.14.1**: [official signed distribution and source](https://www.wintun.net/), supplied under the prebuilt-DLL distribution license copied to `engine/WINTUN-LICENSE.txt`. The installer verifies the archive against the SHA256 published by the Wintun project. Used by sing-box for Windows TUN.
- **Tauri 2 / Wry / Tao**: MIT or Apache-2.0; https://github.com/tauri-apps/tauri
- **React**: MIT; https://github.com/facebook/react
- **Lucide icons**: ISC; https://github.com/lucide-icons/lucide
- **russh**: Apache-2.0; SSH client library, https://github.com/Eugeny/russh. Locked versions and cryptographic dependency licenses are recorded in Cargo.lock.
- **qrcode.react**: ISC; local QR rendering, https://github.com/zpao/qrcode.react.
- **Caddy with Naive forwardproxy v2.11.7-naive**: downloaded on the VPS only during explicit installation; upstream [release and source](https://github.com/klzgrad/forwardproxy/releases/tag/v2.11.7-naive). The upstream archive's LICENSE and README accompany the installed binary. Corepass pins the archive SHA256 and does not alter the binary.
- **Rust dependencies**: versions recorded in Cargo.lock; their upstream license notices apply.

Router engine images contain the same unmodified upstream Linux-musl executable, stored in a read-only compressed SquashFS image. Corepass scripts, UI and original source are MIT; the engine remains under its own license.
