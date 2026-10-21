# Self-host server controller (0.4 preview)

The desktop Rust backend sends this fixed Python 3 controller over a verified,
fingerprint-pinned SSH connection. It accepts a bounded JSON request on stdin and
returns JSON. It installs no public management API. SSH passwords are handled by
the desktop transport, never received or saved by this script.

Initial supported target: **Ubuntu 22.04 / 24.04, Linux amd64, systemd, root SSH**.
The domain must resolve directly to the SSH target, including every A/AAAA record.
Ports 80 and 443 must be free on a fresh installation. Existing Amnezia, S-UI,
Docker, web servers, firewall, SSH settings and DNS configuration are not changed.

## Operations

Each request includes `action`, `host` and `domain`.

- `inspect`: read-only checks; `ready`, `installed`, `service`, `checks`, `blockers`.
- `install`: reruns checks, installs the pinned server if clear; idempotent for a
  matching owned installation. Creates one initial client, returns **public** data.
- `clients.list`: public `id`, `name`, `created_at`, `revoked`, plus service state.
- `clients.create`: also accepts `client_name`; explicit result `client` includes URI.
- `clients.export`: accepts `client_id`; explicit result `client` includes URI.
- `clients.revoke`: accepts `client_id`, gracefully reloads owned Caddy, public list.

`create` / `export` are the only responses that contain access credentials, inside
`client.uri`. A revoked entry cannot be exported. Revocation blocks **new** sessions;
existing tunnels may remain until they close because the service reload is graceful.
The final revoked client is replaced by a random inaccessible authentication entry,
so an empty account list never creates an open proxy.

## Supply chain and isolation

Pinned upstream: [klzgrad/forwardproxy v2.11.7-naive](https://github.com/klzgrad/forwardproxy/releases/tag/v2.11.7-naive).

- Artifact: `caddy-forwardproxy-naive.tar.xz`, 13,305,656 bytes.
- Archive SHA256: `ac97057c925bb098eb575e7b0cb8bd547d4c5f845b09a3dedfbe34444aa7ae88`.
- Official release digest and actual downloaded bytes were checked during development.
- Only the four actual regular/directory archive members are accepted; no tar extraction
  paths or symlinks are followed. ELF64 / little-endian / amd64 header is checked.
- No unpinned fallback, shell interpolation, caller download URLs or apt operations.

Owned tree: `/opt/corepass-server`. Unit: `corepass-server.service`.
Root stores client state and Caddyfile with mode 0600. The daemon runs as a systemd
`DynamicUser` with `LoadCredential`, private runtime socket, its own state directory
and only the capability to bind privileged ports. No static OS user is created.
The public side serves HTTPS/HTTP2; the administrative socket is Unix-only inside
`/run/corepass-server` mode 0700. TLS issuance/renewal belongs to Caddy under that
isolated service identity. Certificate issuance is initially pending and requires
working public reachability and ACME access; an active unit is not proof of usable TLS.

Client edits use syntax adaptation, a transaction marker, previous config/state
backups, an atomic config replacement, and graceful reload of the fixed owned
Unix socket. A failed reload restores old configuration. An interrupted or failed
rollback leaves a recovery marker and blocks later edits; manual inspection is
required rather than silently assuming success. No full application/user-server
rollback is promised after a client SSH session disappears.

The exact generated Caddyfile and systemd unit are compared before management;
manual changes stop automatic writes. Initial-install rollback confirms the own
service has stopped before removing its runtime tree. An unconfirmed stop leaves
the tree/unit and a recovery marker in place instead of deleting a live binary.

## Checks

```sh
python -m unittest discover -s server/tests -v
```

These tests use temporary directories and mocked commands/DNS. They never connect
to an SSH server, configure a service, or activate VPN.

On Linux amd64 CI, also run as an ordinary user:

```sh
python server/tests/check_caddy.py
```

This downloads the pinned archive and asks the real Caddy binary to **adapt** three
generated example configurations. It opens no listening socket and performs no
ACME issuance or service installation. Actual fresh-server installation, public
TLS issuance, client interoperability and performance remain separate live tests.
