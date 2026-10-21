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

## Existing S-UI management

The optional desktop-only `sui` feature supports the S-UI 1.6.5 token API. Saved panel reads and settings writes are local. Explicit HTTPS requests use the `Token` header, certificate validation, no redirects, no system proxy, timeouts and bounded response bodies. Tokens can be operation-scoped or explicitly saved in the current Windows user's generic credential vault under an owned per-panel target. They are never serialized into app files. Re-entering a saved panel's API URL first forgets its stored credential; name/read-only/public-VPN edits preserve it. Secret panel paths and exported client credentials are omitted from inventory, logs and support reports. The independent `sui_` operation queue clears results after five minutes and never acquires the VPN command lock or changes its cancellation state.

Only Naive clients are supported. Public export endpoints are explicit configuration, never the S-UI inbound's internal listen port. Default read-only policy is checked before remote requests for creation; creation also requires per-call confirmation. S-UI may recreate the Naive inbound and close active sessions because its user hot-update switch does not handle Naive. Never test creation against an active user session. A possibly committed POST is not retried automatically.

Run `cargo test -p corepass-sui`, `cargo test -p corepass-core --features selfhost,sui`, and frontend tests/build. The integration tests inject an in-memory S-UI transport and temporary local inventory; they must not use saved user credentials or real remote endpoints. Native startup still has normal proxy-recovery side effects, so packaging the new executable is separate from launching it. Stable 0.3.0, its shortcut and the running client must remain untouched while the user is gaming.

A separate read-only compatibility probe read existing S-UI settings over previously pinned SSH/SFTP and fetched client/inbound summaries and details through valid HTTPS. No remote write, token creation, service reload or VPN test occurred. This is evidence for the API schema and external endpoint mapping, not an end-to-end test of the new native UI. Local panel preconfiguration and a Windows credential write/readback were performed separately without starting the app; private connection settings and tokens are excluded from source and release bundles.
