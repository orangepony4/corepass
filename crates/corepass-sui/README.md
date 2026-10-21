# Existing S-UI panels

This provider uses the S-UI token API under the panel's own HTTPS path. It does
not install/reconfigure a VPN, change Windows networking, stop an engine, restart
a service or modify an inbound. Constructor, inventory and panel-save operations
are local only. Development tests use a fake transport and temporary directories.

The fixed remote operations are inspection, listing Naive clients, exporting a
client URI and creating a client on one selected Naive inbound. There are no
edit, delete, restart, shell, arbitrary URL or raw configuration routes.

Creation can make S-UI restart the selected Naive inbound internally and interrupt
existing sessions. New saved panels are read-only. Creation requires an explicit
local setting `read_only:false` and `confirm_create:true` on the request. The policy
check occurs before any HTTP request. Changing the setting is itself local only.

`Manager::new(PathBuf)` receives a private directory protected by the caller.
`Manager::request(method,path,&Value)` returns final JSON; the desktop core wraps
long operations and owns result expiry and credential storage policy. Public
`validate_request` lets callers reject invalid requests before starting jobs.

- `GET /v1/sui/panels`: public metadata, omitting the private panel path.
- `POST /v1/sui/panels/save`: new records require `name` and `base_url`. Optional
  `server_domain` defaults to its host, `server_port` to 443, `read_only` to true.
  Updates support `panel_id` and partial fields, avoiding exposing/re-entering the
  secret panel path. Endpoint changes reset read-only unless explicitly supplied.
- `POST /v1/sui/inspect` and `/clients/list`: `panel_id`, `token`.
- `POST /v1/sui/clients/export`: add numeric `client_id` and `inbound_id`.
- `POST /v1/sui/clients/create`: `panel_id`, `token`, numeric `inbound_id`,
  `client_name`, `confirm_create:true`.

The token is raw, without a Bearer prefix. This crate never persists it; the
desktop wrapper may separately offer explicit credential-vault storage. The
caller must erase its request copy and avoid logging bodies. Own transient tokens,
generated credentials and sensitive JSON responses are zeroized here.

HTTPS uses normal certificate validation, no system/environment proxy, no
redirects and no automatic retries. Responses are bounded to 2 MiB; connection
timeout is 5 seconds and request timeout 20 seconds. Every envelope must contain
`success:true`, even with HTTP 200. Error messages omit raw bodies and full URLs.

S-UI client `name` is a subscription secret. Public rows show its display `remark`
and exclude subscription IDs/credentials. Only explicit create/export operations
return a personal URI under `client.uri`. The public endpoint must match safe,
TLS-enabled inbound `addrs`/`out_json`, including the domain and external port.
Internal `listen_port` and `tls_id` never determine that endpoint. This supports
plaintext loopback Naive behind HTTPS Caddy. QUIC, insecure TLS and differing SNI
are rejected. Links have `endpoint_verified:false`; VPN connectivity is not tested.

Creation sends exactly one `object=clients,action=new` form POST with independently
random subscription ID, Naive username and password. It attaches only the chosen
inbound. Full readback then verifies enabled state, inbound membership and exact
persisted credentials before returning a URI. Ambiguous write/readback returns
`CP-SUI-CREATE-UNKNOWN` and is never retried automatically.
