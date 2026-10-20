"""Bounded Corepass server commands, sent over a pinned SSH connection.

No network listener or remote management HTTP API is installed. Input is one JSON
object on stdin; output is one JSON object. `inspect` never writes files.
"""
import datetime
import hashlib
import ipaddress
import json
import os
from pathlib import Path
import platform
import re
import secrets
import shutil
import socket
import stat
import struct
import subprocess
import sys
import tarfile
import tempfile
import urllib.parse
import urllib.request

VERSION = "0.4.0"
PRODUCT = "corepass-server-v1"
CADDY_VERSION = "v2.11.7-naive"
CADDY_URL = "https://github.com/klzgrad/forwardproxy/releases/download/v2.11.7-naive/caddy-forwardproxy-naive.tar.xz"
CADDY_SHA256 = "ac97057c925bb098eb575e7b0cb8bd547d4c5f845b09a3dedfbe34444aa7ae88"
CADDY_MEMBER = "caddy-forwardproxy-naive/caddy"
MAX_ARCHIVE = 32 * 1024 * 1024
MAX_BINARY = 100 * 1024 * 1024
MAX_INPUT = 16384
CLIENT_LIMIT = 64
SERVICE_USER = "corepass-server"
UNIT_NAME = "corepass-server.service"
DOMAIN_RE = re.compile(r"(?=.{1,253}\Z)(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z](?:[a-z0-9-]{0,61}[a-z0-9])?\Z")
ID_RE = re.compile(r"[a-f0-9]{32}\Z")


class Failure(Exception):
    def __init__(self, code, message):
        super().__init__(message)
        self.code, self.message = code, message


def fail(code, message):
    raise Failure(code, message)


def validate_request(request):
    if not isinstance(request, dict):
        fail("CP-SERVER-INPUT", "Ожидался JSON-объект.")
    action = request.get("action")
    allowed = {"inspect", "install", "clients.list", "clients.create", "clients.revoke", "clients.export"}
    if action not in allowed:
        fail("CP-SERVER-ACTION", "Неизвестное действие управления сервером.")
    keys = {"action", "domain", "host"}
    if action == "clients.create":
        keys.add("client_name")
    if action in {"clients.export", "clients.revoke"}:
        keys.add("client_id")
    if set(request) - keys:
        fail("CP-SERVER-INPUT", "Неизвестные поля команды.")
    domain = request.get("domain", "")
    if not isinstance(domain, str) or not DOMAIN_RE.fullmatch(domain):
        fail("CP-SERVER-DOMAIN", "Укажите домен латиницей без протокола, порта и пути.")
    host = request.get("host", "")
    if not isinstance(host, str) or len(host) > 253:
        fail("CP-SERVER-HOST", "Некорректный адрес SSH-сервера.")
    try:
        ipaddress.ip_address(host)
    except ValueError:
        if not DOMAIN_RE.fullmatch(host):
            fail("CP-SERVER-HOST", "Некорректный адрес SSH-сервера.")
    if action == "clients.create":
        name = request.get("client_name")
        if not isinstance(name, str) or not 1 <= len(name) <= 64 or any(ord(c) < 32 or ord(c) == 127 for c in name):
            fail("CP-SERVER-CLIENT", "Имя клиента должно содержать от 1 до 64 символов без управляющих знаков.")
    if action in {"clients.export", "clients.revoke"} and not ID_RE.fullmatch(str(request.get("client_id", ""))):
        fail("CP-SERVER-CLIENT", "Некорректный идентификатор клиента.")
    return action


def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds")


def public_client(client):
    return {key: client[key] for key in ("id", "name", "created_at", "revoked")}


def new_client(name):
    return {"id": secrets.token_hex(16), "name": name, "created_at": now(), "revoked": False,
            "username": secrets.token_hex(16), "password": secrets.token_hex(32)}


def client_uri(domain, client):
    if client["revoked"]:
        fail("CP-SERVER-REVOKED", "Доступ этого клиента отозван.")
    return "naive+https://{}:{}@{}:443#{}".format(client["username"], client["password"], domain,
                                                  urllib.parse.quote(client["name"], safe=""))


def caddyfile(domain, clients, empty_auth=None):
    # Domain and credentials are validated/generated internally; display names
    # never enter the server configuration.
    if not DOMAIN_RE.fullmatch(domain):
        fail("CP-SERVER-DOMAIN", "Некорректный домен сервера.")
    enabled = [c for c in clients if not c["revoked"]]
    if not enabled:
        # Keep authentication mandatory even after the final client is revoked.
        enabled = [empty_auth or {"username": secrets.token_hex(32), "password": secrets.token_hex(32)}]
    auth = []
    for client in enabled:
        if not re.fullmatch(r"[a-f0-9]{32,64}", client["username"]) or not re.fullmatch(r"[a-f0-9]{64}", client["password"]):
            fail("CP-SERVER-STATE", "Повреждены данные доступа сервера.")
        auth.append("        basic_auth {} {}".format(client["username"], client["password"]))
    return ("{\n    admin unix//run/corepass-server/admin.sock\n"
            "    order forward_proxy before file_server\n"
            "    servers {\n        protocols h1 h2\n    }\n"
            "    log {\n        exclude http.log.error\n    }\n}\n"
            ":443, " + domain + " {\n    forward_proxy {\n" + "\n".join(auth) +
            "\n        hide_ip\n        hide_via\n        probe_resistance\n    }\n"
            "    root * /opt/corepass-server/site\n    file_server\n}\n")


UNIT = """[Unit]
Description=Corepass private HTTPS server
After=network-online.target
Wants=network-online.target
StartLimitIntervalSec=60
StartLimitBurst=5

[Service]
Type=notify
User=corepass-server
Group=corepass-server
DynamicUser=yes
LoadCredential=Caddyfile:/opt/corepass-server/Caddyfile
ExecStart=/opt/corepass-server/caddy run --config ${CREDENTIALS_DIRECTORY}/Caddyfile --adapter caddyfile
TimeoutStopSec=30
Restart=on-failure
RestartSec=5
Environment=HOME=/var/lib/corepass-server
Environment=XDG_DATA_HOME=/var/lib/corepass-server
Environment=XDG_CONFIG_HOME=/var/lib/corepass-server
StateDirectory=corepass-server
StateDirectoryMode=0700
RuntimeDirectory=corepass-server
RuntimeDirectoryMode=0700
AmbientCapabilities=CAP_NET_BIND_SERVICE
CapabilityBoundingSet=CAP_NET_BIND_SERVICE
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectControlGroups=true
RestrictSUIDSGID=true
LockPersonality=true
UMask=0077
LimitNOFILE=65536
StandardOutput=null
StandardError=journal

[Install]
WantedBy=multi-user.target
"""

SITE = """<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Private workspace</title><style>body{font:16px system-ui;background:#10171a;color:#e8eee9;max-width:680px;margin:20vh auto;padding:32px}small{color:#a1b3a8}h1{font-weight:500}</style><small>PERSONAL WORKSPACE</small><h1>A quiet place on the web.</h1><p>This private workspace is maintained by its owner.</p></html>
"""


def run(arguments, *, timeout=30, check=False):
    # Never run user input as shell syntax or return raw command output.
    result = subprocess.run(arguments, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                            stderr=subprocess.DEVNULL, timeout=timeout, check=False)
    if check and result.returncode:
        fail("CP-SERVER-SERVICE", "Команда службы Corepass завершилась ошибкой.")
    return result.returncode


def atomic_write(path, payload, mode=0o600, gid=None):
    path = Path(path)
    if path.is_symlink():
        fail("CP-SERVER-OWNERSHIP", "Обнаружена символическая ссылка в файлах Corepass.")
    fd, temp = tempfile.mkstemp(prefix=".corepass-", dir=path.parent)
    try:
        if hasattr(os, "fchmod"):
            os.fchmod(fd, mode)
        if gid is not None:
            if hasattr(os, "fchown"):
                os.fchown(fd, 0, gid)
        with os.fdopen(fd, "wb") as output:
            output.write(payload.encode("utf8") if isinstance(payload, str) else payload)
            output.flush()
            os.fsync(output.fileno())
        os.replace(temp, path)
    finally:
        if os.path.exists(temp):
            os.unlink(temp)


class Controller:
    def __init__(self, base=Path("/opt/corepass-server"), unit=Path("/etc/systemd/system/corepass-server.service")):
        self.base, self.unit = Path(base), Path(unit)

    def read_state(self, domain):
        for path in (self.base, self.base / "owner.json", self.base / "state.json", self.base / "caddy", self.base / "Caddyfile", self.unit):
            if path.is_symlink():
                fail("CP-SERVER-OWNERSHIP", "Файлы Corepass подменены символическими ссылками.")
        try:
            owner = json.loads((self.base / "owner.json").read_text(encoding="utf8"))
            state = json.loads((self.base / "state.json").read_text(encoding="utf8"))
        except (OSError, ValueError):
            fail("CP-SERVER-OWNERSHIP", "Не найдена подтверждённая установка Corepass.")
        if owner.get("product") != PRODUCT or state.get("schema") != 1 or state.get("domain") != domain:
            fail("CP-SERVER-OWNERSHIP", "Установка Corepass принадлежит другому домену или имеет неизвестный формат.")
        if self.unit.read_text(encoding="utf8") != UNIT:
            fail("CP-SERVER-OWNERSHIP", "Служба Corepass была изменена вне приложения. Автоматическая запись остановлена.")
        clients = state.get("clients")
        if not isinstance(clients, list) or not 1 <= len(clients) <= CLIENT_LIMIT:
            fail("CP-SERVER-STATE", "Повреждён список клиентов Corepass.")
        for client in clients:
            if not isinstance(client, dict) or not ID_RE.fullmatch(str(client.get("id", ""))):
                fail("CP-SERVER-STATE", "Повреждены данные клиента Corepass.")
            if not isinstance(client.get("name"), str) or not isinstance(client.get("revoked"), bool) or not isinstance(client.get("created_at"), str):
                fail("CP-SERVER-STATE", "Повреждены данные клиента Corepass.")
            caddyfile(domain, [client])
        empty_auth = state.get("empty_auth")
        if not isinstance(empty_auth, dict):
            fail("CP-SERVER-STATE", "Повреждён резерв аутентификации Corepass.")
        caddyfile(domain, [], empty_auth)
        if (self.base / "transaction.json").exists() or (self.base / "install-recovery.json").exists():
            fail("CP-SERVER-RECOVERY", "Предыдущее изменение не завершилось. Нужна проверка резервной копии Corepass.")
        if (self.base / "Caddyfile").read_text(encoding="utf8") != caddyfile(domain, clients, empty_auth):
            fail("CP-SERVER-OWNERSHIP", "Конфигурация Corepass изменена вне приложения. Автоматическая запись остановлена.")
        return state

    def inspect(self, request):
        checks = []
        def add(key, label, ok, message):
            checks.append({"id": key, "label": label, "ok": bool(ok), "message": message})
        add("root", "Права администратора", getattr(os, "geteuid", lambda: -1)() == 0,
            "Нужен SSH-вход от root; sudo и пароль не передаются в оболочку.")
        info = {}
        try:
            for line in Path("/etc/os-release").read_text().splitlines():
                if "=" in line:
                    key, value = line.split("=", 1)
                    info[key] = value.strip('"')
        except OSError:
            pass
        os_name = info.get("ID", "unknown") + " " + info.get("VERSION_ID", "")
        add("os", "Операционная система", info.get("ID") == "ubuntu" and info.get("VERSION_ID") in {"22.04", "24.04"},
            "Первый выпуск поддерживает Ubuntu 22.04 и 24.04.")
        arch = platform.machine()
        add("architecture", "Архитектура сервера", arch in {"x86_64", "amd64"},
            "Проверенный официальный пакет Caddy Naive доступен для amd64.")
        add("systemd", "Управление службой", shutil.which("systemctl") is not None and Path("/run/systemd/system").is_dir(),
            "Нужен сервер с работающим systemd.")
        try:
            free = shutil.disk_usage(self.base.parent).free
        except OSError:
            free = 0
        add("disk", "Свободное место", free >= 180 * 1024 * 1024, "Нужно не менее 180 МБ свободного места в /opt.")
        installed = False
        if self.base.exists() or self.base.is_symlink() or self.unit.exists() or self.unit.is_symlink():
            try:
                self.read_state(request["domain"])
                installed = True
                add("ownership", "Установка Corepass", True, "Найдена собственная установка этого домена.")
            except (Failure, OSError):
                add("ownership", "Установка Corepass", False, "Пути Corepass заняты неизвестными или незавершёнными файлами.")
        else:
            add("ownership", "Установка Corepass", True, "Пути новой службы свободны.")
        try:
            host_addresses = {str(ipaddress.ip_address(item[4][0])) for item in socket.getaddrinfo(request["host"], None, type=socket.SOCK_STREAM)}
            domain_addresses = {str(ipaddress.ip_address(item[4][0])) for item in socket.getaddrinfo(request["domain"], None, type=socket.SOCK_STREAM)}
            dns_ok = bool(host_addresses and domain_addresses) and domain_addresses.issubset(host_addresses)
        except (OSError, ValueError):
            dns_ok = False
        add("dns", "Домен направлен на сервер", dns_ok,
            "Все записи A и AAAA домена должны указывать на адрес SSH-сервера; проксирование CDN нужно выключить.")
        for port in (80, 443):
            free_port = True
            if not installed:
                for family, address in ((socket.AF_INET, "0.0.0.0"), (socket.AF_INET6, "::")):
                    try:
                        with socket.socket(family, socket.SOCK_STREAM) as probe:
                            if family == socket.AF_INET6:
                                probe.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_V6ONLY, 1)
                            probe.bind((address, port))
                    except OSError as exc:
                        # An OS without IPv6 is valid; an occupied port is not.
                        if family == socket.AF_INET6 and exc.errno in {97, 99}:
                            continue
                        free_port = False
            add("port_" + str(port), "Порт " + str(port), free_port,
                "Порт используется собственной службой Corepass." if installed else
                "Порт должен быть свободен. Другие VPN, панели и веб-серверы не останавливаются.")
        service = "active" if installed and run(["systemctl", "is-active", "--quiet", UNIT_NAME]) == 0 else "inactive"
        return {"ready": all(c["ok"] for c in checks), "installed": installed, "domain": request["domain"], "host": request["host"],
                "architecture": arch, "os": os_name, "service": service, "checks": checks,
                "blockers": [c["id"] for c in checks if not c["ok"]], "version": VERSION}

    def download_binary(self, stage):
        archive = stage / "caddy.tar.xz"
        digest, size = hashlib.sha256(), 0
        # Fixed HTTPS URL only; no caller-controlled download endpoint.
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        with opener.open(urllib.request.Request(CADDY_URL, headers={"User-Agent": "Corepass/" + VERSION}), timeout=45) as response, archive.open("wb") as output:
            if urllib.parse.urlsplit(response.geturl()).scheme != "https":
                fail("CP-SERVER-DOWNLOAD", "Загрузка перенаправлена на небезопасный адрес.")
            while True:
                chunk = response.read(1024 * 1024)
                if not chunk:
                    break
                size += len(chunk)
                if size > MAX_ARCHIVE:
                    fail("CP-SERVER-DOWNLOAD", "Размер пакета сервера превысил предел.")
                digest.update(chunk)
                output.write(chunk)
        if digest.hexdigest() != CADDY_SHA256:
            fail("CP-SERVER-CHECKSUM", "Контрольная сумма официального пакета не совпала.")
        with tarfile.open(archive, "r:xz") as source:
            members = source.getmembers()
            allowed = {"caddy-forwardproxy-naive", CADDY_MEMBER, "caddy-forwardproxy-naive/LICENSE", "caddy-forwardproxy-naive/README.md"}
            if len(members) > 4 or any(m.name.rstrip("/") not in allowed or not (m.isfile() or m.isdir()) for m in members):
                fail("CP-SERVER-ARCHIVE", "Неожиданные файлы в официальном пакете.")
            selected = [m for m in members if m.name == CADDY_MEMBER and m.isfile()]
            if len(selected) != 1 or not 20 <= selected[0].size <= MAX_BINARY:
                fail("CP-SERVER-ARCHIVE", "Некорректный исполняемый файл в пакете.")
            binary = source.extractfile(selected[0]).read(MAX_BINARY + 1)
            if len(binary) != selected[0].size or binary[:6] != b"\x7fELF\x02\x01" or struct.unpack_from("<H", binary, 18)[0] != 62:
                fail("CP-SERVER-ARCHIVE", "Пакет не содержит проверенный Linux amd64 ELF.")
            atomic_write(stage / "caddy", binary, 0o755)
            for name in ("LICENSE", "README.md"):
                member = next((m for m in members if m.name == "caddy-forwardproxy-naive/" + name and m.isfile() and m.size < 1024 * 1024), None)
                if member is None:
                    fail("CP-SERVER-ARCHIVE", "В пакете отсутствуют лицензия или описание.")
                atomic_write(stage / name, source.extractfile(member).read(), 0o644)
        archive.unlink()

    def install(self, request):
        status = self.inspect(request)
        if not status["ready"]:
            fail("CP-SERVER-PREFLIGHT", "Установка остановлена: не пройдены проверки " + ", ".join(status["blockers"]) + ".")
        if status["installed"]:
            return {"installed": True, "already_installed": True, "domain": request["domain"], "service": status["service"],
                    "clients": [public_client(c) for c in self.read_state(request["domain"])["clients"]]}
        # Linux only, imported lazily so pure validation can be tested on Windows.
        import fcntl
        lock_fd = os.open("/run/corepass-install.lock", os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
        stage = None
        promoted = False
        unit_written = False
        try:
            try:
                fcntl.flock(lock_fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                fail("CP-SERVER-BUSY", "Другая установка Corepass ещё выполняется.")
            # A second process may have changed ownership or ports while downloading.
            status = self.inspect(request)
            if not status["ready"] or status["installed"]:
                fail("CP-SERVER-PREFLIGHT", "Состояние сервера изменилось. Повторите проверку.")
            stage = Path(tempfile.mkdtemp(prefix=".corepass-stage-", dir=self.base.parent))
            self.download_binary(stage)
            client = new_client("Это устройство")
            reserve = new_client("disabled-reserve")
            state = {"schema": 1, "domain": request["domain"], "version": VERSION, "created_at": now(), "clients": [client],
                     "empty_auth": {key: reserve[key] for key in ("username", "password")}}
            atomic_write(stage / "state.json", json.dumps(state, ensure_ascii=False), 0o600)
            atomic_write(stage / "owner.json", json.dumps({"product": PRODUCT, "caddy_version": CADDY_VERSION, "archive_sha256": CADDY_SHA256}), 0o644)
            atomic_write(stage / "Caddyfile", caddyfile(request["domain"], state["clients"], state["empty_auth"]), 0o600)
            (stage / "site").mkdir(mode=0o755)
            atomic_write(stage / "site/index.html", SITE, 0o644)
            # Adapt checks syntax without provisioning ACME/storage as root.
            # Runtime validation happens in the isolated service before ready.
            run([str(stage / "caddy"), "adapt", "--config", str(stage / "Caddyfile"), "--adapter", "caddyfile"], check=True)
            status = self.inspect(request)
            if not status["ready"] or status["installed"]:
                fail("CP-SERVER-PREFLIGHT", "Порты или файлы сервера изменились. Ничего не включалось.")
            stage.chmod(0o755)
            os.rename(stage, self.base)
            promoted = True
            # Exclusive creation prevents replacing a foreign unit after inspection.
            fd = os.open(self.unit, os.O_CREAT | os.O_EXCL | os.O_WRONLY | os.O_NOFOLLOW, 0o644)
            with os.fdopen(fd, "w", encoding="utf8") as output:
                output.write(UNIT)
                output.flush()
                os.fsync(output.fileno())
            unit_written = True
            run(["systemctl", "daemon-reload"], check=True)
            run(["systemctl", "enable", "--now", UNIT_NAME], timeout=45, check=True)
            if run(["systemctl", "is-active", "--quiet", UNIT_NAME]) != 0:
                fail("CP-SERVER-SERVICE", "Новая служба не запустилась. Выполнен откат собственной установки.")
            return {"installed": True, "domain": request["domain"], "service": "active", "certificate": "automatic_pending",
                    "clients": [public_client(client)], "version": VERSION}
        except Exception as original:
            if not self.rollback_install(unit_written, promoted):
                fail("CP-SERVER-RECOVERY", "Не удалось подтвердить остановку и откат собственной службы. Файлы сохранены для проверки; повторная установка заблокирована.")
            raise original
        finally:
            if stage is not None and stage.exists():
                shutil.rmtree(stage)
            os.close(lock_fd)

    def rollback_install(self, unit_written, promoted):
        """Never remove a binary/config while its owned daemon may still run."""
        safe_to_remove = not unit_written
        try:
            if unit_written:
                if self.unit.is_symlink() or self.unit.read_text(encoding="utf8") != UNIT:
                    return self.install_recovery_marker()
                stopped = run(["systemctl", "disable", "--now", UNIT_NAME], timeout=45) == 0
                # systemctl is-active: 3 means known but inactive/failed.
                inactive = run(["systemctl", "is-active", "--quiet", UNIT_NAME], timeout=10) == 3
                safe_to_remove = stopped and inactive
                if not safe_to_remove:
                    return self.install_recovery_marker()
                self.unit.unlink()
                run(["systemctl", "daemon-reload"], timeout=15)
            if promoted and safe_to_remove:
                shutil.rmtree(self.base)
            return True
        except Exception:
            return self.install_recovery_marker()

    def install_recovery_marker(self):
        try:
            if self.base.is_dir() and not self.base.is_symlink():
                atomic_write(self.base / "install-recovery.json", json.dumps({"created_at": now(), "reason": "rollback_unconfirmed"}))
        except Exception:
            pass
        return False

    def commit_clients(self, previous, clients):
        # Serialize read/modify/reload per installed server. The caller holds flock.
        config_path = self.base / "Caddyfile"
        before = config_path.read_bytes()
        candidate = self.base / "Caddyfile.candidate"
        new_state = dict(previous, clients=clients)
        atomic_write(candidate, caddyfile(previous["domain"], clients, previous["empty_auth"]), 0o600)
        reload_command = [str(self.base / "caddy"), "reload", "--config", str(config_path), "--adapter", "caddyfile", "--address", "unix//run/corepass-server/admin.sock"]
        try:
            if run([str(self.base / "caddy"), "adapt", "--config", str(candidate), "--adapter", "caddyfile"]) != 0:
                fail("CP-SERVER-CONFIG", "Новая конфигурация не прошла проверку. Доступы сохранены.")
            # Marker survives interruption and makes later commands fail closed.
            atomic_write(self.base / "state.previous.json", json.dumps(previous, ensure_ascii=False))
            atomic_write(self.base / "Caddyfile.previous", before, 0o600)
            atomic_write(self.base / "transaction.json", json.dumps({"started_at": now()}))
            os.replace(candidate, config_path)
            if run(reload_command) != 0:
                atomic_write(config_path, before, 0o600)
                restored = run(reload_command) == 0
                if restored:
                    (self.base / "transaction.json").unlink()
                fail("CP-SERVER-RELOAD" if restored else "CP-SERVER-RECOVERY",
                     "Изменение не применено. Предыдущая конфигурация восстановлена." if restored else
                     "Служба не приняла изменение или откат. Резервная копия сохранена, нужна диагностика Corepass.")
            atomic_write(self.base / "state.json", json.dumps(new_state, ensure_ascii=False))
            (self.base / "transaction.json").unlink()
        finally:
            candidate.unlink(missing_ok=True)

    def clients(self, request):
        state = self.read_state(request["domain"])
        action = request["action"]
        if action == "clients.list":
            return {"clients": [public_client(c) for c in state["clients"]], "domain": state["domain"],
                    "service": "active" if run(["systemctl", "is-active", "--quiet", UNIT_NAME]) == 0 else "inactive"}
        if action == "clients.export":
            target = next((c for c in state["clients"] if c["id"] == request["client_id"]), None)
            if target is None:
                fail("CP-SERVER-NOT-FOUND", "Клиент не найден.")
            return {"client": dict(public_client(target), uri=client_uri(state["domain"], target))}
        import fcntl
        lock_fd = os.open(self.base / "operation.lock", os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
        try:
            try:
                fcntl.flock(lock_fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                fail("CP-SERVER-BUSY", "Изменение клиентов уже выполняется.")
            state = self.read_state(request["domain"])
            clients = [dict(c) for c in state["clients"]]
            if action == "clients.create":
                if len(clients) >= CLIENT_LIMIT:
                    fail("CP-SERVER-LIMIT", "Достигнут предел 64 записей клиентов.")
                client = new_client(request["client_name"])
                clients.append(client)
                self.commit_clients(state, clients)
                return {"client": dict(public_client(client), uri=client_uri(state["domain"], client))}
            target = next((c for c in clients if c["id"] == request["client_id"]), None)
            if target is None:
                fail("CP-SERVER-NOT-FOUND", "Клиент не найден.")
            if not target["revoked"]:
                target["revoked"] = True
                self.commit_clients(state, clients)
            return {"clients": [public_client(c) for c in clients]}
        finally:
            os.close(lock_fd)

    def execute(self, request):
        action = validate_request(request)
        if action == "inspect":
            return self.inspect(request)
        if getattr(os, "geteuid", lambda: -1)() != 0:
            fail("CP-SERVER-ROOT", "Для управления установкой нужен root.")
        if action == "install":
            return self.install(request)
        return self.clients(request)


def main():
    try:
        incoming = sys.stdin.buffer.read(MAX_INPUT + 1)
        if len(incoming) > MAX_INPUT:
            fail("CP-SERVER-INPUT", "Команда превышает допустимый размер.")
        request = json.loads(incoming)
        result = Controller().execute(request)
        output = dict(result, ok=True)
    except Failure as exc:
        output = {"ok": False, "error": {"code": exc.code, "message": exc.message}}
    except (ValueError, TypeError, UnicodeError):
        output = {"ok": False, "error": {"code": "CP-SERVER-INPUT", "message": "Некорректная команда или данные сервера."}}
    except (OSError, subprocess.SubprocessError):
        output = {"ok": False, "error": {"code": "CP-SERVER-SYSTEM", "message": "Операция сервера не завершилась. Личные данные не включены в ответ."}}
    except Exception:
        output = {"ok": False, "error": {"code": "CP-SERVER-INTERNAL", "message": "Внутренняя ошибка управления сервером. Не повторяйте установку без проверки состояния."}}
    print(json.dumps(output, ensure_ascii=False, separators=(",", ":")))


if __name__ == "__main__":
    main()
