# Corepass SSH control plane

This crate owns server provisioning and client management. It never changes the
Windows proxy, TUN, firewall, DNS, client engine process or router configuration.
Constructing `Manager` and listing local servers do not open a socket. Callers
must execute network requests on their own bounded worker and must not log input
JSON bodies. The local API is authenticated by the desktop core.

`Manager::new(data: PathBuf) -> Result<Manager, String>` takes a directory protected
by the caller. The only persistent data is `servers.json`, containing public VPS
metadata and its pinned SSH fingerprint. The caller also owns password input
lifetime and must erase its request JSON after completion. The crate wraps its
own password copy in `Zeroizing` and does not log library or command errors.

`validate_request(method, path, &body)` rejects unknown fields before networking.
`Manager::request(method, path, &body)` validates again and returns final JSON.

1. `POST /v1/servers/host-key`: `host`, optional `ssh_port` (default 22). Records
   the signed SSH host key exchange and rejects authentication. Returns a SHA256
   fingerprint and algorithm. It does not establish that the observed key belongs
   to the intended VPS; the user must compare it with a trusted host fingerprint.
2. `POST /v1/servers/inspect`: `host`, `ssh_port`, `username`, `password`, `domain`,
   `fingerprint`. Password authentication happens only after an exact fingerprint
   match. Runs read-only checks through the bundled Python controller. Returns a
   10-minute `plan_id`, checks and blockers.
3. `POST /v1/servers/install`: same fields plus `plan_id`. The plan is bound to the
   host, SSH port, root username, domain and pinned fingerprint. It is single-use.
   The backend rechecks the VPS before any mutation. Install returns only public
   client metadata; sharing credentials requires an explicit export request.
4. `GET /v1/servers`: public inventory; no network request.
5. `POST /v1/servers/clients/list`: `server_id`, `password`.
6. `POST /v1/servers/clients/create`: add `client_name`.
7. `POST /v1/servers/clients/revoke` or `/export`: add `client_id`.

Only explicit create/export responses contain a client URI. The calling core
must expire these asynchronous results and omit them from support reports.

The SSH command is a fixed, short Python bootstrap. Bundled source is transferred
as a byte-length-prefixed frame on stdin, immediately followed by the user JSON.
The bootstrap and controller read the same buffered stdin stream. No source or
user values enter shell interpolation. There is no general exec,
script URL or custom command route. Output is capped at 256 KiB, stderr at 32 KiB,
connection at 15 seconds, authentication at 20 seconds, normal operations at 60
seconds and installation at 15 minutes. A timeout is an ambiguous remote outcome;
the user must inspect the VPS before retrying. The timeout is not a promise that
the remote process rolled back or stopped.

The first server controller supports root SSH access to Ubuntu 22.04/24.04 amd64.
No SSH or live VPN tests are included in this iteration. Unit tests use a fake
transport and temporary directories; they do not access a user's AppData or VPS.
