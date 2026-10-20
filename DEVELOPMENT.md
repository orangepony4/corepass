# Corepass Next

## Scope

1. Rust + Tauri 2 Windows application, React/TypeScript UI, existing official sing-box engine.
2. Independent app data and ports; read-only detection of Corepass 0.2. Never stop or reconfigure the active client during development.
3. OpenWrt package for Cudy WR3000S v1 / 24.10: lightweight LuCI UI, disabled on installation, preserve Steer/WARP/DoH/AWG.
4. GitHub source, tested release artifacts, one-command router installation with checksum verification.

## Verification boundary

Windows networking activation must not be tested while the user watches a film through Corepass 0.2.
Router firmware/hardware has not been accessed; installer and runtime need offline tests before publishing and a real-device validation later.
No private profiles, subscription links, API tokens, server access details or local logs belong in this repository.
