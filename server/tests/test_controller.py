"""Offline contract/security tests. No SSH, socket connection or service changes."""
import copy
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import socket
import sys
import tarfile
import tempfile
import types
import unittest
from unittest.mock import MagicMock, patch

SCRIPT = Path(__file__).resolve().parents[1] / "controller.py"
spec = importlib.util.spec_from_file_location("corepass_server_controller", SCRIPT)
controller = importlib.util.module_from_spec(spec)
spec.loader.exec_module(controller)


class InputTests(unittest.TestCase):
    def test_allowlist_and_domain_injection(self):
        for action in ("exec", "shell", "restart", "delete", None):
            with self.assertRaises(controller.Failure):
                controller.validate_request({"action": action, "domain": "example.com", "host": "192.0.2.1"})
        for domain in ("x.com; rm -rf /", "https://example.com", "x.com\nadmin :2019", "192.0.2.1", "*.example.com", "EXAMPLE.com", "localhost", "a..example.com"):
            with self.assertRaises(controller.Failure):
                controller.validate_request({"action": "inspect", "domain": domain, "host": "192.0.2.1"})
        with self.assertRaises(controller.Failure):
            controller.validate_request({"action": "inspect", "domain": "example.com", "host": "192.0.2.1", "command": "whoami"})

    def test_display_names_never_enter_caddy_config(self):
        client = controller.new_client('" }\nforward_proxy {\n# harmless display name')
        config = controller.caddyfile("example.com", [client])
        self.assertNotIn(client["name"], config)
        self.assertIn("basic_auth " + client["username"] + " " + client["password"], config)

    def test_public_client_does_not_contain_access(self):
        client = controller.new_client("iPhone")
        public = json.dumps(controller.public_client(client))
        self.assertNotIn(client["password"], public)
        self.assertNotIn(client["username"], public)
        self.assertNotIn("uri", public)
        self.assertEqual(len(client["password"]), 64)
        self.assertNotEqual(client["password"], controller.new_client("Other")["password"])

    def test_revoking_last_client_does_not_open_proxy(self):
        client = controller.new_client("Last")
        client["revoked"] = True
        config = controller.caddyfile("example.com", [client])
        self.assertIn("basic_auth ", config)
        self.assertNotIn(client["username"], config)
        self.assertNotIn(client["password"], config)
        self.assertIn("probe_resistance", config)
        with self.assertRaises(controller.Failure):
            controller.client_uri("example.com", client)

    def test_uri_encodes_only_display_fragment(self):
        client = controller.new_client("Телефон #1 / тест")
        uri = controller.client_uri("vpn.example.com", client)
        self.assertTrue(uri.startswith("naive+https://"))
        self.assertIn("@vpn.example.com:443#", uri)
        self.assertNotIn(" ", uri)
        self.assertNotIn("Телефон", uri)

    def test_service_is_private_and_hardened(self):
        self.assertIn("User=corepass-server", controller.UNIT)
        self.assertIn("DynamicUser=yes", controller.UNIT)
        self.assertIn("LoadCredential=Caddyfile:", controller.UNIT)
        self.assertNotIn("ExecReload=+", controller.UNIT)
        self.assertIn("NoNewPrivileges=true", controller.UNIT)
        self.assertIn("ProtectSystem=strict", controller.UNIT)
        config = controller.caddyfile("example.com", [controller.new_client("Device")])
        self.assertIn("admin unix//run/corepass-server/admin.sock", config)
        self.assertNotIn(":2019", config)
        self.assertNotIn("h3", config)


class OwnedStateTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.base = self.root / "corepass-server"
        self.base.mkdir()
        self.unit = self.root / "corepass-server.service"
        self.agent = controller.Controller(self.base, self.unit)
        self.client = controller.new_client("Device")
        reserve = controller.new_client("disabled-reserve")
        self.state = {"schema": 1, "domain": "vpn.example.com", "clients": [self.client],
                      "empty_auth": {key: reserve[key] for key in ("username", "password")}}
        (self.base / "owner.json").write_text(json.dumps({"product": controller.PRODUCT}))
        (self.base / "state.json").write_text(json.dumps(self.state))
        (self.base / "caddy").write_bytes(b"test binary, never executed")
        (self.base / "Caddyfile").write_text(controller.caddyfile(self.state["domain"], [self.client]))
        self.unit.write_text(controller.UNIT)

    def tearDown(self):
        self.tmp.cleanup()

    def test_foreign_domain_and_marker_are_rejected(self):
        with self.assertRaises(controller.Failure):
            self.agent.read_state("other.example.com")
        (self.base / "owner.json").write_text('{"product":"other"}')
        with self.assertRaises(controller.Failure):
            self.agent.read_state("vpn.example.com")

    def test_changed_unit_is_not_overwritten(self):
        self.unit.write_text("foreign service")
        with self.assertRaises(controller.Failure) as error:
            self.agent.read_state("vpn.example.com")
        self.assertEqual(error.exception.code, "CP-SERVER-OWNERSHIP")
        self.assertEqual(self.unit.read_text(), "foreign service")

    def test_changed_configuration_is_not_overwritten(self):
        with (self.base / "Caddyfile").open("a") as config:
            config.write("\n# manual change\n")
        with self.assertRaises(controller.Failure) as error:
            self.agent.read_state("vpn.example.com")
        self.assertEqual(error.exception.code, "CP-SERVER-OWNERSHIP")

    def test_partial_transaction_blocks_later_changes(self):
        (self.base / "transaction.json").write_text('{}')
        with self.assertRaises(controller.Failure) as error:
            self.agent.read_state("vpn.example.com")
        self.assertEqual(error.exception.code, "CP-SERVER-RECOVERY")

    def test_list_is_public_and_export_is_explicit(self):
        with patch.object(controller, "run", return_value=0):
            listed = self.agent.clients({"action": "clients.list", "domain": "vpn.example.com", "host": "192.0.2.1"})
        self.assertNotIn(self.client["password"], json.dumps(listed))
        self.assertNotIn("uri", json.dumps(listed))
        exported = self.agent.clients({"action": "clients.export", "domain": "vpn.example.com", "host": "192.0.2.1", "client_id": self.client["id"]})
        self.assertIn(self.client["password"], exported["client"]["uri"])
        self.assertNotIn("password", exported["client"])

    def test_failed_validation_keeps_old_config_and_state(self):
        before = (self.base / "Caddyfile").read_bytes()
        clients = [dict(self.client, revoked=True)]
        with patch.object(controller, "run", return_value=1):
            with self.assertRaises(controller.Failure) as error:
                self.agent.commit_clients(self.state, clients)
        self.assertEqual(error.exception.code, "CP-SERVER-CONFIG")
        self.assertEqual((self.base / "Caddyfile").read_bytes(), before)
        self.assertEqual(json.loads((self.base / "state.json").read_text()), self.state)
        self.assertFalse((self.base / "transaction.json").exists())

    def test_failed_reload_restores_previous_access(self):
        before = (self.base / "Caddyfile").read_bytes()
        with patch.object(controller, "run", side_effect=[0, 1, 0]) as runner:
            with self.assertRaises(controller.Failure) as error:
                self.agent.commit_clients(self.state, [dict(self.client, revoked=True)])
        self.assertEqual(error.exception.code, "CP-SERVER-RELOAD")
        self.assertEqual((self.base / "Caddyfile").read_bytes(), before)
        self.assertEqual(json.loads((self.base / "state.json").read_text()), self.state)
        reload = runner.call_args_list[1].args[0]
        self.assertEqual(reload[:2], [str(self.base / "caddy"), "reload"])
        self.assertEqual(reload[-2:], ["--address", "unix//run/corepass-server/admin.sock"])
        self.assertFalse((self.base / "transaction.json").exists())

    def test_failed_reload_and_rollback_keep_recovery_marker(self):
        with patch.object(controller, "run", side_effect=[0, 1, 1]):
            with self.assertRaises(controller.Failure) as error:
                self.agent.commit_clients(self.state, [dict(self.client, revoked=True)])
        self.assertEqual(error.exception.code, "CP-SERVER-RECOVERY")
        self.assertTrue((self.base / "transaction.json").is_file())
        self.assertTrue((self.base / "state.previous.json").is_file())

    def test_successful_reload_updates_state_atomically(self):
        with patch.object(controller, "run", return_value=0):
            self.agent.commit_clients(self.state, [dict(self.client, revoked=True)])
        state = self.agent.read_state("vpn.example.com")
        self.assertTrue(state["clients"][0]["revoked"])
        self.assertNotIn(self.client["username"], (self.base / "Caddyfile").read_text())
        self.assertFalse((self.base / "transaction.json").exists())

    def test_install_with_blocker_never_downloads_or_creates(self):
        with patch.object(self.agent, "inspect", return_value={"ready": False, "blockers": ["port_443"], "installed": False}), \
             patch.object(self.agent, "download_binary") as download, patch.object(controller, "run") as runner:
            with self.assertRaises(controller.Failure) as error:
                self.agent.install({"domain": "vpn.example.com", "host": "192.0.2.1"})
        self.assertEqual(error.exception.code, "CP-SERVER-PREFLIGHT")
        download.assert_not_called()
        runner.assert_not_called()

    def test_owned_install_is_idempotent_and_never_exports_credentials(self):
        with patch.object(self.agent, "inspect", return_value={"ready": True, "installed": True, "service": "active"}), \
             patch.object(self.agent, "download_binary") as download, patch.object(controller, "run") as runner:
            result = self.agent.install({"domain": "vpn.example.com", "host": "192.0.2.1"})
        self.assertTrue(result["already_installed"])
        self.assertNotIn(self.client["password"], json.dumps(result))
        download.assert_not_called()
        runner.assert_not_called()

    def test_install_rollback_timeout_preserves_runtime_and_marker(self):
        import subprocess
        with patch.object(controller, "run", side_effect=subprocess.TimeoutExpired("systemctl", 45)):
            self.assertFalse(self.agent.rollback_install(True, True))
        self.assertTrue(self.unit.exists())
        self.assertTrue((self.base / "caddy").exists())
        self.assertTrue((self.base / "install-recovery.json").exists())
        with self.assertRaises(controller.Failure) as error:
            self.agent.read_state("vpn.example.com")
        self.assertEqual(error.exception.code, "CP-SERVER-RECOVERY")

    def test_install_rollback_confirms_inactive_before_removing(self):
        with patch.object(controller, "run", side_effect=[0, 3, 0]):
            self.assertTrue(self.agent.rollback_install(True, True))
        self.assertFalse(self.unit.exists())
        self.assertFalse(self.base.exists())

    def test_install_rollback_preserves_foreign_changed_unit(self):
        self.unit.write_text("foreign replacement")
        with patch.object(controller, "run") as runner:
            self.assertFalse(self.agent.rollback_install(True, True))
        runner.assert_not_called()
        self.assertEqual(self.unit.read_text(), "foreign replacement")
        self.assertTrue((self.base / "caddy").exists())


class InspectionTests(unittest.TestCase):
    def inspect_fake(self, dns=None, occupied=None, arch="x86_64"):
        with tempfile.TemporaryDirectory() as tmp:
            base = Path(tmp) / "corepass-server"
            unit = Path(tmp) / "corepass-server.service"
            agent = controller.Controller(base, unit)
            original_read = Path.read_text
            original_isdir = Path.is_dir
            def read(path, *args, **kwargs):
                if str(path).replace("\\", "/") == "/etc/os-release":
                    return 'ID=ubuntu\nVERSION_ID="24.04"\n'
                return original_read(path, *args, **kwargs)
            def isdir(path):
                return True if str(path).replace("\\", "/") == "/run/systemd/system" else original_isdir(path)
            def resolve(host, *args, **kwargs):
                addresses = (dns or {}).get(host, ["192.0.2.1"])
                return [(socket.AF_INET6 if ":" in address else socket.AF_INET, socket.SOCK_STREAM, 6, "", (address, 0)) for address in addresses]
            def sock(*args, **kwargs):
                fake = MagicMock()
                fake.__enter__.return_value = fake
                def bind(address):
                    if address[1] == occupied:
                        raise OSError(98, "Address already in use")
                fake.bind.side_effect = bind
                return fake
            with patch.object(Path, "read_text", read), patch.object(Path, "is_dir", isdir), \
                 patch.object(controller.os, "geteuid", return_value=0, create=True), \
                 patch.object(controller.platform, "machine", return_value=arch), \
                 patch.object(controller.shutil, "which", return_value="/bin/tool"), \
                 patch.object(controller.shutil, "disk_usage", return_value=types.SimpleNamespace(free=200 * 1024 * 1024)), \
                 patch.object(controller.socket, "getaddrinfo", side_effect=resolve), patch.object(controller.socket, "socket", side_effect=sock), \
                 patch.object(controller, "atomic_write") as write, patch.object(controller, "run") as runner, \
                 patch.object(controller.tempfile, "mkdtemp") as mkdir:
                result = agent.inspect({"domain": "vpn.example.com", "host": "192.0.2.1"})
            write.assert_not_called()
            runner.assert_not_called()
            mkdir.assert_not_called()
            self.assertEqual(list(Path(tmp).iterdir()), [])
            return result

    def test_read_only_ready_inspect(self):
        result = self.inspect_fake()
        self.assertTrue(result["ready"])
        self.assertFalse(result["installed"])

    def test_occupied_port_is_blocker(self):
        result = self.inspect_fake(occupied=443)
        self.assertFalse(result["ready"])
        self.assertIn("port_443", result["blockers"])

    def test_foreign_aaaa_record_blocks_install(self):
        result = self.inspect_fake(dns={"vpn.example.com": ["192.0.2.1", "2001:db8::2"]})
        self.assertIn("dns", result["blockers"])

    def test_unverified_arm64_artifact_is_not_used(self):
        result = self.inspect_fake(arch="aarch64")
        self.assertIn("architecture", result["blockers"])


class DownloadTests(unittest.TestCase):
    def archive(self, malicious=False):
        data = io.BytesIO()
        with tarfile.open(fileobj=data, mode="w:xz") as archive:
            for name, content in [(controller.CADDY_MEMBER, b"\x7fELF\x02\x01" + b"\0" * 12 + b"\x3e\0"),
                                  ("caddy-forwardproxy-naive/LICENSE", b"license"),
                                  ("caddy-forwardproxy-naive/README.md", b"source")]:
                member = tarfile.TarInfo(name)
                member.size = len(content)
                archive.addfile(member, io.BytesIO(content))
            if malicious:
                member = tarfile.TarInfo("../../outside")
                member.size = 1
                archive.addfile(member, io.BytesIO(b"x"))
        return data.getvalue()

    def run_fake_download(self, payload, digest):
        with tempfile.TemporaryDirectory() as tmp:
            response = io.BytesIO(payload)
            response.geturl = lambda: controller.CADDY_URL
            opener = MagicMock()
            opener.open.return_value = response
            agent = controller.Controller(Path(tmp) / "base", Path(tmp) / "unit")
            with patch.object(controller.urllib.request, "build_opener", return_value=opener), \
                 patch.object(controller, "CADDY_SHA256", digest):
                agent.download_binary(Path(tmp))
            self.assertEqual((Path(tmp) / "caddy").read_bytes()[:4], b"\x7fELF")
            self.assertTrue((Path(tmp) / "LICENSE").exists())

    def test_official_pinned_digest_required(self):
        with self.assertRaises(controller.Failure) as error:
            self.run_fake_download(self.archive(), "0" * 64)
        self.assertEqual(error.exception.code, "CP-SERVER-CHECKSUM")

    def test_archive_path_traversal_rejected_before_extract(self):
        payload = self.archive(malicious=True)
        with self.assertRaises(controller.Failure) as error:
            self.run_fake_download(payload, hashlib.sha256(payload).hexdigest())
        self.assertEqual(error.exception.code, "CP-SERVER-ARCHIVE")

    def test_verified_allowlisted_archive(self):
        payload = self.archive()
        self.run_fake_download(payload, hashlib.sha256(payload).hexdigest())


if __name__ == "__main__":
    unittest.main()
