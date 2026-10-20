# Development and verification

The desktop and router controllers share a fixed-schema Naive HTTPS profile parser. Subscription-provided routing, DNS, executable paths and inbound configuration are discarded.

Windows data and ports are independent of Corepass 0.2. Detection of the older API prevents activation of a second VPN. Test import and API behavior before any network-changing checks. Preserve the original client as a fallback.

Run `cargo test -p corepass-core -p corepass-router`, `npm test`, and `npm run build`. Native Windows checks must additionally exercise system proxy restoration, process termination recovery and real HTTPS through TUN.

The GitHub workflow builds on Windows and native ARM64 Linux. Router checks include a static-musl controller, actual sing-box configuration validation, and execution from the mounted read-only SquashFS image. The installer archive has an explicit regular-file allowlist and never contains private settings.

Hardware checks on the Cudy remain a separate step: available memory under load, selected-device IPv4/IPv6/DNS routing, exclusion of other devices, reboot recovery, and coexistence with the installed firewall rules. Do not equate a successful ARM64 build with a completed router hardware test.

Never commit private profiles, subscription URLs, API tokens, access details, local application data or diagnostic reports.

## 0.4 preview verification boundary

The self-host control plane is an optional Rust core feature enabled by the desktop crate. It is excluded from the router controller. SSH runs through russh with an exact SHA256 host-key pin checked before password authentication. Passwords are scoped to an operation. Inventory reads are local only; provisioning and client changes require explicit commands. Server operations have their own busy flag and in-memory results, independent from local tunnel state and cancellation.

Run `cargo test -p corepass-selfhost`, `cargo test -p corepass-core --features selfhost`, `python -m unittest discover -s server/tests -v`, `npm test`, and `npm run build`. The SSH tests use a fake transport; server tests use temporary directories and mocked commands. Tests must not authenticate to a real VPS or modify its services. Browser preview fixtures exercise the interface without native commands. Do not launch a new native build, switch the system proxy, activate TUN, change routes, or restart the user's current client while the user is gaming.

The first installer deliberately supports only Ubuntu 22.04/24.04 amd64 with root SSH and a directly pointed domain. It refuses occupied HTTPS/ACME ports and foreign files, verifies a pinned upstream archive, and owns one dedicated systemd service. It does not migrate an existing S-UI/Amnezia deployment. Native Windows regression tests for the new binary and a complete real-VPS install/ACME/client/revoke test remain required before calling this a stable release. Re-running 0.3 tests is not evidence for 0.4 runtime behavior.

Transport work remains separate from the application control plane. A unique wire format is not itself evidence of resistance to DPI. Any future transport needs an explicit threat model, standard cryptography, external review, interoperability tests and measurements across real networks. Do not publish claims of invisibility or guaranteed resistance based on a renamed or lightly modified protocol.
