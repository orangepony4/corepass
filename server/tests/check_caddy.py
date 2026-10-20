"""Verify real pinned Caddy parses generated configs; never listen or install.

Run on Linux amd64 CI as an ordinary user. An optional --binary path must point
to the independently verified official binary; default downloads the fixed pin.
"""
import argparse
import base64
import importlib.util
import json
from pathlib import Path
import platform
import subprocess
import tempfile

spec = importlib.util.spec_from_file_location("corepass_server_controller", Path(__file__).resolve().parents[1] / "controller.py")
controller = importlib.util.module_from_spec(spec)
spec.loader.exec_module(controller)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path)
    options = parser.parse_args()
    if platform.system() != "Linux" or platform.machine() not in {"x86_64", "amd64"}:
        raise SystemExit("This check requires Linux amd64; it does not configure a host.")
    with tempfile.TemporaryDirectory(prefix="corepass-caddy-check-") as temporary:
        stage = Path(temporary)
        if options.binary:
            binary = options.binary.resolve(strict=True)
        else:
            controller.Controller().download_binary(stage)
            binary = stage / "caddy"
        clients = [controller.new_client("Example phone"), controller.new_client("Example laptop")]
        for case, records in (("two_clients", clients), ("one_revoked", [dict(clients[0], revoked=True), clients[1]]),
                              ("all_revoked", [dict(c, revoked=True) for c in clients])):
            config = stage / (case + ".Caddyfile")
            config.write_text(controller.caddyfile("vpn.example.com", records), encoding="utf8")
            result = subprocess.run([str(binary), "adapt", "--config", str(config), "--adapter", "caddyfile"],
                                    capture_output=True, timeout=20)
            if result.returncode:
                # Only generated example credentials are used in this test.
                raise AssertionError(case + ": " + result.stderr.decode("utf8", errors="replace")[:4096])
            parsed = json.loads(result.stdout)
            assert parsed["admin"]["listen"] == "unix//run/corepass-server/admin.sock"
            text = json.dumps(parsed)
            assert '"forward_proxy"' in text
            assert '"probe_resistance"' in text
            def walk(value):
                if isinstance(value, dict):
                    yield value
                    for child in value.values():
                        yield from walk(child)
                elif isinstance(value, list):
                    for child in value:
                        yield from walk(child)
            handlers = [value for value in walk(parsed) if value.get("handler") == "forward_proxy"]
            assert handlers
            # Go [][]byte JSON base64 wraps the plugin's own base64 credentials.
            credentials = {base64.b64decode(base64.b64decode(encoded)).decode("ascii")
                           for handler in handlers for encoded in handler.get("auth_credentials", [])}
            assert credentials, "Even all-revoked configuration must require authentication"
            for client in records:
                if client["revoked"]:
                    assert client["username"] + ":" + client["password"] not in credentials
                else:
                    assert client["username"] + ":" + client["password"] in credentials
            print(case + ": actual pinned Caddy adapt passed", flush=True)
    print("No listening sockets, service installation, SSH or VPN activation were performed.")


if __name__ == "__main__":
    main()
