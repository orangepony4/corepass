# Development and verification

The desktop and router controllers share a fixed-schema Naive HTTPS profile parser. Subscription-provided routing, DNS, executable paths and inbound configuration are discarded.

Windows data and ports are independent of Corepass 0.2. Detection of the older API prevents activation of a second VPN. Test import and API behavior before any network-changing checks. Preserve the original client as a fallback.

Run `cargo test -p corepass-core -p corepass-router`, `npm test`, and `npm run build`. Native Windows checks must additionally exercise system proxy restoration, process termination recovery and real HTTPS through TUN.

The GitHub workflow builds on Windows and native ARM64 Linux. Router checks include a static-musl controller, actual sing-box configuration validation, and execution from the mounted read-only SquashFS image. The installer archive has an explicit regular-file allowlist and never contains private settings.

Hardware checks on the Cudy remain a separate step: available memory under load, selected-device IPv4/IPv6/DNS routing, exclusion of other devices, reboot recovery, and coexistence with the installed firewall rules. Do not equate a successful ARM64 build with a completed router hardware test.

Never commit private profiles, subscription URLs, API tokens, access details, local application data or diagnostic reports.
