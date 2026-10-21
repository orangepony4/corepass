import {
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import {
  ArrowDownToLine,
  ArrowLeft,
  ArrowRight,
  Check,
  CheckCircle2,
  ChevronRight,
  Copy,
  Eye,
  EyeOff,
  Fingerprint,
  Globe2,
  KeyRound,
  LoaderCircle,
  LockKeyhole,
  Plus,
  RefreshCw,
  Server,
  ShieldCheck,
  Smartphone,
  Trash2,
  UserRound,
  Users,
  X,
  AlertCircle,
  Cable,
} from "lucide-react";
import { QRCodeSVG } from "qrcode.react";
import { api, message, native } from "./api";
import {
  connectionError,
  runOperation,
  type ClientExport,
  type ConnectionForm,
  type Inspection,
  type ManagedServer,
  type ServerClient,
} from "./server-api";
import "./selfhost.css";

const preview = !native
  ? new URLSearchParams(location.search).get("preview")
  : null;
const exampleServer: ManagedServer = {
  id: "preview-server",
  name: "Мой сервер",
  host: "203.0.113.10",
  ssh_port: 22,
  username: "root",
  domain: "vpn.example.com",
  fingerprint: "SHA256:9sHdwVkzKeTYZ14hAGhPss0nlWbwN0NfBPhBCIYxYxc",
  created_at: "2026-10-21T12:00:00Z",
};
const exampleClients: ServerClient[] = [
  { id: "desktop", name: "Мой компьютер", created_at: "2026-10-21T12:05:00Z" },
  { id: "iphone", name: "iPhone", created_at: "2026-10-21T12:06:00Z" },
];
const exampleExport: ClientExport = {
  id: "iphone",
  name: "iPhone",
  uri: "naive+https://preview:example-only@vpn.example.com:443#iPhone",
};
const examplePlan: Inspection = {
  plan_id: "preview",
  ready: true,
  installed: false,
  expires_in_seconds: 600,
  host: exampleServer.host,
  domain: exampleServer.domain,
  architecture: "x86_64",
  os: "Ubuntu 24.04",
  blockers: [],
  checks: [
    {
      id: "os",
      label: "Система поддерживается",
      ok: true,
      message: "Ubuntu 24.04 · amd64",
    },
    {
      id: "dns",
      label: "Домен указывает на сервер",
      ok: true,
      message: "vpn.example.com → 203.0.113.10",
    },
    {
      id: "ports",
      label: "Порты 80 и 443 свободны",
      ok: true,
      message: "Можно получить сертификат и запустить HTTPS",
    },
    {
      id: "services",
      label: "Установке ничего не мешает",
      ok: true,
      message: "Существующие службы сохранят свои настройки",
    },
  ],
};

export function SelfHost({
  active,
  onImport,
  onNotice,
  onBusyChange,
}: {
  active: boolean;
  onImport: (uri: string) => Promise<boolean>;
  onNotice: (text: string) => void;
  onBusyChange: (busy: boolean) => void;
}) {
  const [servers, setServers] = useState<ManagedServer[]>(
    preview === "server" || preview === "share" ? [exampleServer] : [],
  );
  const [loading, setLoading] = useState(native);
  const [error, setError] = useState("");
  const [wizard, setWizard] = useState(
    preview === "setup" || preview === "review",
  );
  const [selected, setSelected] = useState<ManagedServer | null>(
    preview === "server" || preview === "share" ? exampleServer : null,
  );
  const [clients, setClients] = useState<ServerClient[]>(
    preview === "server" || preview === "share" ? exampleClients : [],
  );
  const [listed, setListed] = useState(
    Boolean(preview === "server" || preview === "share"),
  );
  const [access, setAccess] = useState<{
    action: "list" | "create" | "revoke" | "export";
    client?: ServerClient;
  } | null>(null);
  const [share, setShare] = useState<ClientExport | null>(
    preview === "share" ? exampleExport : null,
  );
  const refresh = async () => {
    if (!native) return;
    setLoading(true);
    try {
      const value = await api<{ servers: ManagedServer[] }>(
        "GET",
        "/v1/servers",
      );
      setServers(value.servers);
      setError("");
    } catch (e) {
      setError(message(e));
    } finally {
      setLoading(false);
    }
  };
  useEffect(() => {
    void refresh();
  }, []);
  const manage = (server: ManagedServer) => {
    setSelected(server);
    setClients([]);
    setListed(false);
  };
  if (wizard)
    return (
      <ServerWizard
        initialPreview={preview}
        onBusyChange={onBusyChange}
        onClose={() => setWizard(false)}
        onComplete={(server, installedClients) => {
          setWizard(false);
          setSelected(server);
          setClients(installedClients ?? []);
          setListed(Boolean(installedClients));
          void refresh();
          onNotice(
            "Сервер добавлен в управление. HTTPS-сертификат может ещё выпускаться.",
          );
        }}
      />
    );
  return (
    <div className="selfhost-page">
      {error && <InlineError text={error} />}
      {servers.length === 0 && !loading ? (
        <>
          <section className="selfhost-hero card">
            <div className="selfhost-hero-copy">
              <span className="eyebrow">
                <span className="sh-mini-dot" /> SELF-HOSTED · PREVIEW
              </span>
              <h2>
                Ваш сервер.
                <br />
                <span>Ваши правила.</span>
              </h2>
              <p>
                Настройте личный VPN прямо из Corepass.
                <br />
                Приложение подготовит сервер, а вы сможете
                <br className="sh-wide-break" /> создавать доступ для своих
                устройств.
              </p>
              <button
                className="button primary sh-hero-button"
                onClick={() => setWizard(true)}
              >
                <Plus size={18} /> Настроить свой сервер{" "}
                <ArrowRight size={17} />
              </button>
              <span className="sh-hero-foot">
                <LockKeyhole size={13} /> Без регистрации и внешней панели
              </span>
            </div>
            <ServerArt />
          </section>
          <div className="sh-section-heading">
            <h3>От сервера до подключения — три шага</h3>
            <span>Всё в одном приложении</span>
          </div>
          <div className="sh-steps-grid">
            {[
              {
                n: "01",
                icon: Server,
                title: "Добавьте свой VPS",
                text: "IP, SSH-пароль и домен. Сначала проверим сервер и покажем план установки.",
              },
              {
                n: "02",
                icon: ShieldCheck,
                title: "Corepass подготовит VPN",
                text: "Установит HTTPS-сервер и настроит сертификат после вашего подтверждения.",
              },
              {
                n: "03",
                icon: Users,
                title: "Пригласите устройства",
                text: "Отдельный доступ для каждого клиента: ссылка, QR-код и отзыв в один шаг.",
              },
            ].map((item) => (
              <article className="card sh-step-card" key={item.n}>
                <span className="sh-step-number">{item.n}</span>
                <span className="sh-icon-box">
                  <item.icon size={22} />
                </span>
                <h3>{item.title}</h3>
                <p>{item.text}</p>
              </article>
            ))}
          </div>
          <div className="sh-requirements">
            <Globe2 size={17} />
            <p>
              Для первой версии: чистый{" "}
              <strong>Ubuntu 22.04 / 24.04 · amd64</strong>, SSH-доступ root и
              домен, направленный на IP сервера. Порты 80 и 443 должны быть
              свободны.
            </p>
            <span className="sh-beta">PREVIEW</span>
          </div>
        </>
      ) : (
        <>
          <div className="sh-section-heading sh-top-toolbar">
            <div>
              <h3>Серверы под вашим управлением</h3>
              <p>Управление по SSH · пароль запрашивается перед действием</p>
            </div>
            <button className="button primary" onClick={() => setWizard(true)}>
              <Plus size={16} /> Добавить сервер
            </button>
          </div>
          <div className="sh-managed-layout">
            <div className="sh-server-list">
              {loading && !servers.length && (
                <div className="sh-loading">
                  <LoaderCircle className="sh-spinner" /> Загружаем список…
                </div>
              )}
              {servers.map((server) => (
                <button
                  key={server.id}
                  className={`card sh-managed-card ${selected?.id === server.id ? "selected" : ""}`}
                  onClick={() => manage(server)}
                >
                  <span className="sh-icon-box">
                    <Server size={22} />
                  </span>
                  <div>
                    <strong>{server.name || server.domain}</strong>
                    <span>{server.domain}</span>
                    <small>
                      {server.host} · SSH {server.ssh_port}
                    </small>
                  </div>
                  <ChevronRight size={18} />
                </button>
              ))}
              <div className="sh-local-note">
                <LockKeyhole size={14} /> Пароль SSH не сохраняется на диске.
              </div>
            </div>
            {selected ? (
              <section className="card sh-clients-panel">
                <div className="sh-panel-heading">
                  <div>
                    <span className="eyebrow">УПРАВЛЕНИЕ СЕРВЕРОМ</span>
                    <h2>{selected.domain}</h2>
                    <p>
                      <Server size={13} /> {selected.host} <span>·</span> Naive
                      HTTPS
                    </p>
                  </div>
                  <span className="sh-beta">PREVIEW</span>
                </div>
                <div className="sh-client-toolbar">
                  <h3>
                    <Users size={18} /> Клиенты{" "}
                    {listed && <span>{clients.length}</span>}
                  </h3>
                  <div>
                    <button
                      className="icon-button"
                      aria-label="Обновить клиентов с сервера"
                      disabled={!native}
                      onClick={() => setAccess({ action: "list" })}
                    >
                      <RefreshCw size={17} />
                    </button>
                    <button
                      className="button primary"
                      disabled={!native}
                      onClick={() => setAccess({ action: "create" })}
                    >
                      <Plus size={16} /> Новый клиент
                    </button>
                  </div>
                </div>
                {!listed ? (
                  <div className="sh-clients-empty">
                    <KeyRound size={30} />
                    <h3>Получите список клиентов</h3>
                    <p>
                      Подключимся к серверу по SSH.
                      <br />
                      VPN на этом компьютере не переключается.
                    </p>
                    <button
                      className="button"
                      disabled={!native}
                      onClick={() => setAccess({ action: "list" })}
                    >
                      Открыть управление <ArrowRight size={16} />
                    </button>
                  </div>
                ) : clients.length === 0 ? (
                  <div className="sh-clients-empty">
                    <Users size={30} />
                    <h3>Добавьте первое устройство</h3>
                    <p>Для каждого клиента создаётся отдельный доступ.</p>
                  </div>
                ) : (
                  <div className="sh-clients-list">
                    {clients.map((client) => (
                      <div
                        className={`sh-client-row ${client.revoked ? "revoked" : ""}`}
                        key={client.id}
                      >
                        <span className="sh-client-icon">
                          {/phone|телефон/i.test(client.name) ? (
                            <Smartphone size={20} />
                          ) : (
                            <UserRound size={20} />
                          )}
                        </span>
                        <div className="sh-client-info">
                          <strong>{client.name}</strong>
                          <span>
                            {client.revoked ? "Доступ отозван" : "Доступ выдан"}
                            {client.created_at &&
                              ` · ${new Date(client.created_at).toLocaleDateString("ru-RU")}`}
                          </span>
                        </div>
                        <span
                          className={`sh-client-status ${client.revoked ? "" : "enabled"}`}
                        >
                          <i />
                          {client.revoked ? "Отозван" : "Активен"}
                        </span>
                        <button
                          className="icon-button"
                          aria-label={`Показать QR-код: ${client.name}`}
                          disabled={!native || client.revoked}
                          onClick={() =>
                            setAccess({ action: "export", client })
                          }
                        >
                          <ArrowDownToLine size={17} />
                        </button>
                        <button
                          className="icon-button sh-delete"
                          aria-label={`Отозвать доступ: ${client.name}`}
                          disabled={!native || client.revoked}
                          onClick={() =>
                            setAccess({ action: "revoke", client })
                          }
                        >
                          <Trash2 size={16} />
                        </button>
                      </div>
                    ))}
                  </div>
                )}
                <div className="sh-panel-footer">
                  <ShieldCheck size={15} /> Клиенты управляются на вашем
                  сервере. Облачная панель не нужна.
                </div>
              </section>
            ) : (
              <section className="card sh-clients-empty">
                <Server size={32} />
                <h3>Выберите сервер слева</h3>
                <p>Откройте его, чтобы управлять доступом устройств.</p>
              </section>
            )}
          </div>
        </>
      )}
      {access && selected && (
        <AccessDialog
          active={active}
          action={access.action}
          client={access.client}
          server={selected}
          onBusyChange={onBusyChange}
          onClose={() => setAccess(null)}
          onResult={(value) => {
            if (value.clients) {
              setClients(value.clients);
              setListed(true);
            }
            if (value.client) {
              setShare(value.client);
              if (access.action === "create") {
                setClients((prev) => [
                  ...prev,
                  { id: value.client!.id, name: value.client!.name },
                ]);
              }
            }
            setAccess(null);
            onNotice(
              access.action === "revoke"
                ? "Доступ клиента отозван"
                : access.action === "create"
                  ? "Новый клиент создан"
                  : "Готово",
            );
          }}
        />
      )}
      {share && (
        <ShareDialog
          active={active}
          client={share}
          onClose={() => setShare(null)}
          onImport={onImport}
          onNotice={onNotice}
        />
      )}
    </div>
  );
}

function ServerWizard({
  onClose,
  onComplete,
  initialPreview,
  onBusyChange,
}: {
  onClose: () => void;
  onComplete: (server: ManagedServer, clients?: ServerClient[]) => void;
  initialPreview: string | null;
  onBusyChange: (busy: boolean) => void;
}) {
  const [step, setStep] = useState(initialPreview === "review" ? 2 : 0);
  const [form, setForm] = useState<ConnectionForm>(
    initialPreview
      ? {
          host: exampleServer.host,
          ssh_port: 22,
          username: "root",
          password: "",
          domain: exampleServer.domain,
        }
      : { host: "", ssh_port: 22, username: "root", password: "", domain: "" },
  );
  const [showPassword, setShowPassword] = useState(false);
  const [fingerprint, setFingerprint] = useState("");
  const [algorithm, setAlgorithm] = useState("");
  const [trusted, setTrusted] = useState(false);
  const [reviewed, setReviewed] = useState(false);
  const [inspection, setInspection] = useState<Inspection | null>(
    initialPreview === "review" ? examplePlan : null,
  );
  const [pending, setPending] = useState("");
  const [error, setError] = useState("");
  useEffect(() => {
    onBusyChange(Boolean(pending));
    return () => onBusyChange(false);
  }, [pending, onBusyChange]);
  const update = (key: keyof ConnectionForm, value: string | number) =>
    setForm((prev) => ({ ...prev, [key]: value }));
  async function hostKey(event: FormEvent) {
    event.preventDefault();
    const problem = connectionError(form);
    if (problem) {
      setError(problem);
      return;
    }
    setError("");
    setPending("Получаем ключ SSH-сервера…");
    try {
      const value = await runOperation<{
        fingerprint: string;
        algorithm: string;
      }>("/v1/servers/host-key", {
        host: form.host.trim(),
        ssh_port: form.ssh_port,
      });
      setFingerprint(value.fingerprint);
      setAlgorithm(value.algorithm);
      setTrusted(false);
      setStep(1);
    } catch (e) {
      setError(message(e));
    } finally {
      setPending("");
    }
  }
  async function inspect() {
    if (!trusted || !fingerprint) return;
    setError("");
    setPending("Проверяем сервер и готовим план…");
    try {
      const value = await runOperation<Inspection>("/v1/servers/inspect", {
        ...form,
        host: form.host.trim(),
        domain: form.domain.trim(),
        fingerprint,
      });
      setInspection(value);
      setReviewed(false);
      setStep(2);
    } catch (e) {
      setError(message(e));
    } finally {
      setPending("");
    }
  }
  async function install() {
    if (!inspection?.ready || !reviewed) return;
    setError("");
    setPending("Устанавливаем сервер. Это может занять несколько минут…");
    setStep(3);
    try {
      const value = await runOperation<{
        server: ManagedServer;
        clients?: ServerClient[];
      }>(
        "/v1/servers/install",
        {
          ...form,
          host: form.host.trim(),
          domain: form.domain.trim(),
          fingerprint,
          plan_id: inspection.plan_id,
        },
        { timeoutMs: 17 * 60_000 },
      );
      setForm((prev) => ({ ...prev, password: "" }));
      onComplete(value.server, value.clients);
    } catch (e) {
      setError(message(e));
      setStep(2);
    } finally {
      setPending("");
    }
  }
  const labels = [
    "Доступ к серверу",
    "Ключ SSH",
    "План установки",
    "Установка",
  ];
  return (
    <div className="sh-wizard">
      <div className="sh-wizard-top">
        <button
          className="sh-back"
          disabled={Boolean(pending)}
          onClick={onClose}
        >
          <ArrowLeft size={16} /> Мои серверы
        </button>
        <span className="sh-beta">SELF-HOSTED PREVIEW</span>
      </div>
      <div className="sh-wizard-layout">
        <aside className="sh-wizard-aside">
          <div className="sh-icon-box">
            <Server size={25} />
          </div>
          <h2>
            Настроим
            <br />
            ваш сервер.
          </h2>
          <p>
            Один раз установите VPN.
            <br />
            Дальше управляйте доступом
            <br />
            из Corepass.
          </p>
          <ol>
            {labels.map((label, index) => (
              <li
                className={
                  step === index ? "current" : step > index ? "complete" : ""
                }
                key={label}
              >
                <span>{step > index ? <Check size={13} /> : index + 1}</span>
                <div>
                  {label}
                  <small>
                    {
                      [
                        "IP, пароль и домен",
                        "Подтверждение отпечатка",
                        "Проверка и состав изменений",
                        "Подготовка личного VPN",
                      ][index]
                    }
                  </small>
                </div>
              </li>
            ))}
          </ol>
          <div className="sh-wizard-local">
            <LockKeyhole size={16} />
            <span>
              SSH-пароль нужен для настройки.
              <br />
              Мы не записываем его
              <br />в файлы и журнал.
            </span>
          </div>
        </aside>
        <section className="card sh-wizard-main" aria-busy={Boolean(pending)}>
          {error && <InlineError text={error} />}
          <div key={step} className="sh-step-content">
            {step === 0 && (
              <form onSubmit={hostKey}>
                <span className="eyebrow">ШАГ 1 ИЗ 4</span>
                <h2>Добавьте свой VPS</h2>
                <p className="sh-form-intro">
                  Подойдёт чистый Ubuntu 22.04 или 24.04 на amd64.
                  <br />
                  Для HTTPS нужен домен, который указывает на IP сервера.
                </p>
                <div className="sh-form-grid">
                  <Field label="IP или имя сервера" htmlFor="sh-host">
                    <input
                      id="sh-host"
                      value={form.host}
                      onChange={(e) => update("host", e.target.value)}
                      placeholder="203.0.113.10"
                      autoComplete="off"
                      spellCheck={false}
                      required
                      disabled={Boolean(pending)}
                    />
                  </Field>
                  <Field label="Порт SSH" htmlFor="sh-port">
                    <input
                      id="sh-port"
                      type="number"
                      min="1"
                      max="65535"
                      value={form.ssh_port}
                      onChange={(e) =>
                        update("ssh_port", Number(e.target.value))
                      }
                      required
                      disabled={Boolean(pending)}
                    />
                  </Field>
                </div>
                <Field label="Имя пользователя" htmlFor="sh-user">
                  <input id="sh-user" value="root" readOnly />
                  <small>В первой версии нужен доступ root.</small>
                </Field>
                <Field label="Пароль SSH" htmlFor="sh-password">
                  <div className="sh-password-field">
                    <input
                      id="sh-password"
                      type={showPassword ? "text" : "password"}
                      value={form.password}
                      onChange={(e) => update("password", e.target.value)}
                      autoComplete="off"
                      placeholder="Пароль от вашего VPS"
                      required
                      disabled={Boolean(pending)}
                    />
                    <button
                      type="button"
                      className="icon-button"
                      aria-label={
                        showPassword
                          ? "Скрыть пароль SSH"
                          : "Показать пароль SSH"
                      }
                      onClick={() => setShowPassword(!showPassword)}
                    >
                      {showPassword ? <EyeOff size={17} /> : <Eye size={17} />}
                    </button>
                  </div>
                </Field>
                <Field label="Домен VPN" htmlFor="sh-domain">
                  <input
                    id="sh-domain"
                    value={form.domain}
                    onChange={(e) => update("domain", e.target.value)}
                    autoComplete="off"
                    spellCheck={false}
                    placeholder="vpn.example.com"
                    required
                    disabled={Boolean(pending)}
                  />
                  <small>
                    A-запись домена должна вести на IP сервера. Порты 80 и 443 —
                    свободны.
                  </small>
                </Field>
                <div className="sh-gentle-note">
                  <ShieldCheck size={17} />
                  <p>
                    Сначала покажем ключ SSH и проверим сервер. Установка
                    начнётся отдельным действием.
                  </p>
                </div>
                <div className="sh-form-actions">
                  <span>
                    <LockKeyhole size={13} /> Прямое подключение по SSH
                  </span>
                  <button
                    type="submit"
                    className="button primary"
                    disabled={!native || Boolean(pending)}
                  >
                    {pending ? (
                      <LoaderCircle className="sh-spinner" size={16} />
                    ) : null}
                    {pending ? "Получаем ключ…" : "Продолжить"}
                    <ArrowRight size={16} />
                  </button>
                </div>
              </form>
            )}
            {step === 1 && (
              <>
                <span className="eyebrow">ШАГ 2 ИЗ 4</span>
                <h2>Это ваш сервер?</h2>
                <p className="sh-form-intro">
                  Сверьте отпечаток SSH-ключа с данными VPS-провайдера.
                  <br />
                  Пароль ещё не отправлялся.
                </p>
                <div className="sh-fingerprint-card">
                  <span className="sh-icon-box">
                    <Fingerprint size={25} />
                  </span>
                  <div>
                    <strong>
                      {form.host}:{form.ssh_port}
                    </strong>
                    <small>{algorithm || "Ключ SSH"}</small>
                  </div>
                  <code>{fingerprint}</code>
                </div>
                <label className="sh-check-label">
                  <input
                    type="checkbox"
                    checked={trusted}
                    onChange={(e) => setTrusted(e.target.checked)}
                    disabled={Boolean(pending)}
                  />
                  <span>
                    Отпечаток совпадает. Разрешаю SSH-подключение к этому
                    серверу.
                  </span>
                </label>
                <div className="sh-gentle-note">
                  <KeyRound size={18} />
                  <p>
                    После подтверждения Corepass войдёт по SSH и проверит
                    систему, DNS и доступность портов.
                  </p>
                </div>
                <div className="sh-form-actions">
                  <button
                    className="button"
                    disabled={Boolean(pending)}
                    onClick={() => {
                      setTrusted(false);
                      setStep(0);
                    }}
                  >
                    <ArrowLeft size={15} /> Назад
                  </button>
                  <button
                    className="button primary"
                    disabled={!native || !trusted || Boolean(pending)}
                    onClick={() => void inspect()}
                  >
                    {pending ? (
                      <LoaderCircle className="sh-spinner" size={16} />
                    ) : (
                      <ShieldCheck size={16} />
                    )}
                    {pending ? "Проверяем сервер…" : "Проверить сервер"}
                  </button>
                </div>
              </>
            )}
            {step === 2 && inspection && (
              <>
                <span className="eyebrow">ШАГ 3 ИЗ 4</span>
                <h2>
                  {inspection.ready
                    ? "Всё готово к установке"
                    : "Нужно подготовить сервер"}
                </h2>
                <p className="sh-form-intro">
                  {inspection.host} <span className="sh-plan-arrow">→</span>{" "}
                  {inspection.domain}
                  <br />
                  {inspection.os} · {inspection.architecture}
                </p>
                <div className="sh-preflight">
                  {inspection.checks.map((check) => (
                    <div key={check.id}>
                      <span className={check.ok ? "ok" : "failed"}>
                        {check.ok ? (
                          <CheckCircle2 size={19} />
                        ) : (
                          <AlertCircle size={19} />
                        )}
                      </span>
                      <div>
                        <strong>{check.label}</strong>
                        <p>{check.message}</p>
                      </div>
                    </div>
                  ))}
                </div>
                {inspection.blockers.length > 0 && (
                  <div className="sh-blockers">
                    {inspection.blockers.map((text, i) => (
                      <p key={i}>{text}</p>
                    ))}
                  </div>
                )}
                <div className="sh-change-plan">
                  <h3>Что установит Corepass</h3>
                  <div>
                    <Server size={16} />
                    <span>Naive HTTPS на вашем домене · TCP 443</span>
                  </div>
                  <div>
                    <LockKeyhole size={16} />
                    <span>TLS-сертификат и его автоматическое продление</span>
                  </div>
                  <div>
                    <Users size={16} />
                    <span>
                      Службу Corepass и управление клиентами через SSH
                    </span>
                  </div>
                </div>
                {inspection.ready && (
                  <label className="sh-check-label">
                    <input
                      type="checkbox"
                      checked={reviewed}
                      onChange={(e) => setReviewed(e.target.checked)}
                      disabled={Boolean(pending)}
                    />
                    <span>
                      Установить Corepass на <strong>{inspection.host}</strong>{" "}
                      с доменом <strong>{inspection.domain}</strong>.
                    </span>
                  </label>
                )}
                <div className="sh-form-actions">
                  <button
                    className="button"
                    disabled={Boolean(pending)}
                    onClick={() => {
                      setInspection(null);
                      setTrusted(false);
                      setStep(0);
                    }}
                  >
                    <ArrowLeft size={15} /> Изменить данные
                  </button>
                  <button
                    className="button primary"
                    disabled={
                      !native ||
                      !inspection.ready ||
                      !reviewed ||
                      Boolean(pending)
                    }
                    onClick={() => void install()}
                  >
                    Установить на сервер <ArrowRight size={16} />
                  </button>
                </div>
              </>
            )}
            {step === 3 && (
              <div className="sh-installing">
                <div className="sh-install-orbit">
                  <Server size={40} />
                  <span />
                  <i />
                </div>
                <span className="eyebrow">ШАГ 4 ИЗ 4</span>
                <h2>Готовим ваш личный VPN</h2>
                <p>{pending}</p>
                <div className="sh-indeterminate">
                  <span />
                </div>
                <small>
                  Оставьте приложение открытым до завершения.
                  <br />
                  Подключение VPN на этом компьютере не переключается.
                </small>
              </div>
            )}
          </div>
        </section>
      </div>
    </div>
  );
}

function Field({
  label,
  htmlFor,
  children,
}: {
  label: string;
  htmlFor: string;
  children: ReactNode;
}) {
  return (
    <div className="sh-field">
      <label htmlFor={htmlFor}>{label}</label>
      {children}
    </div>
  );
}
function InlineError({ text }: { text: string }) {
  return (
    <div className="sh-error" role="alert">
      <AlertCircle size={18} />
      <p>{text}</p>
    </div>
  );
}
export function Dialog({
  active,
  title,
  onClose,
  busy = false,
  children,
  className = "",
}: {
  active: boolean;
  title: string;
  onClose: () => void;
  busy?: boolean;
  children: ReactNode;
  className?: string;
}) {
  const ref = useRef<HTMLElement>(null);
  const closeRef = useRef(onClose);
  closeRef.current = onClose;
  const busyRef = useRef(busy);
  busyRef.current = busy;
  useEffect(() => {
    if (!active) return;
    const previous = document.activeElement as HTMLElement | null;
    const dialog = ref.current;
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    const focusable = () =>
      Array.from(
        dialog?.querySelectorAll<HTMLElement>(
          'button:not([disabled]),input:not([disabled]),textarea,[href],[tabindex="0"]',
        ) ?? [],
      );
    (dialog?.querySelector<HTMLElement>("input") ?? focusable()[0])?.focus();
    const keydown = (event: KeyboardEvent) => {
      if (!dialog || dialog.getClientRects().length === 0) return;
      if (event.key === "Escape" && !busyRef.current) {
        event.preventDefault();
        closeRef.current();
      }
      if (event.key === "Tab") {
        const items = focusable();
        const first = items[0];
        const last = items[items.length - 1];
        if (event.shiftKey && document.activeElement === first) {
          event.preventDefault();
          last?.focus();
        } else if (!event.shiftKey && document.activeElement === last) {
          event.preventDefault();
          first?.focus();
        }
      }
    };
    document.addEventListener("keydown", keydown);
    return () => {
      document.removeEventListener("keydown", keydown);
      document.body.style.overflow = previousOverflow;
      if (previous?.isConnected && previous.getClientRects().length)
        previous.focus();
    };
  }, [active]);
  if (!active) return null;
  return createPortal(
    <div
      className="modal-backdrop sh-modal-backdrop"
      onClick={() => !busy && onClose()}
    >
      <section
        ref={ref}
        className={`card sh-dialog ${className}`}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="sh-dialog-heading">
          <h2>{title}</h2>
          <button
            className="icon-button"
            aria-label="Закрыть окно"
            disabled={busy}
            onClick={onClose}
          >
            <X size={20} />
          </button>
        </div>
        {children}
      </section>
    </div>,
    document.body,
  );
}
function AccessDialog({
  active,
  action,
  client,
  server,
  onClose,
  onResult,
  onBusyChange,
}: {
  active: boolean;
  action: "list" | "create" | "revoke" | "export";
  client?: ServerClient;
  server: ManagedServer;
  onClose: () => void;
  onResult: (value: {
    clients?: ServerClient[];
    client?: ClientExport;
  }) => void;
  onBusyChange: (busy: boolean) => void;
}) {
  const [password, setPassword] = useState("");
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    onBusyChange(busy);
    return () => onBusyChange(false);
  }, [busy, onBusyChange]);
  const title = {
    list: "Подключиться к серверу",
    create: "Новый клиент",
    revoke: "Отозвать доступ",
    export: "Показать профиль клиента",
  }[action];
  async function submit(event: FormEvent) {
    event.preventDefault();
    setError("");
    setBusy(true);
    try {
      const value = await runOperation<{
        clients?: ServerClient[];
        client?: ClientExport;
      }>(`/v1/servers/clients/${action}`, {
        server_id: server.id,
        password,
        ...(action === "create" ? { client_name: name.trim() } : {}),
        ...(client ? { client_id: client.id } : {}),
      });
      setPassword("");
      onResult(value);
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  }
  return (
    <Dialog
      active={active}
      title={title}
      onClose={() => {
        setPassword("");
        onClose();
      }}
      busy={busy}
    >
      <form onSubmit={submit}>
        <p className="sh-dialog-intro">
          {action === "revoke" ? (
            <>
              Новые подключения клиента <strong>{client?.name}</strong> будут
              запрещены. Уже открытые соединения могут работать до закрытия.
            </>
          ) : action === "create" ? (
            "Отдельный профиль можно передать ссылкой или QR-кодом."
          ) : (
            <>
              SSH-доступ к <strong>{server.domain}</strong>
            </>
          )}
        </p>
        {error && <InlineError text={error} />}
        {action === "create" && (
          <Field label="Название устройства" htmlFor="client-name">
            <input
              id="client-name"
              placeholder="Например, iPhone Михаила"
              value={name}
              onChange={(e) => setName(e.target.value)}
              maxLength={80}
              required
              disabled={busy}
            />
          </Field>
        )}
        <Field
          label={`Пароль SSH · ${server.username}@${server.host}`}
          htmlFor="server-password"
        >
          <input
            id="server-password"
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            autoComplete="off"
            required
            disabled={busy}
          />
        </Field>
        <div className="sh-secret-note">
          <LockKeyhole size={14} /> Пароль не сохраняется. SSH-ключ сервера
          будет проверен.
        </div>
        <div className="sh-form-actions">
          <button
            type="button"
            className="button"
            onClick={() => {
              setPassword("");
              onClose();
            }}
            disabled={busy}
          >
            Отмена
          </button>
          <button
            type="submit"
            className={`button ${action === "revoke" ? "sh-danger-button" : "primary"}`}
            disabled={
              !native ||
              busy ||
              !password ||
              (action === "create" && !name.trim())
            }
          >
            {busy && <LoaderCircle className="sh-spinner" size={16} />}
            {busy
              ? "Выполняем…"
              : action === "create"
                ? "Создать клиента"
                : action === "revoke"
                  ? "Отозвать доступ"
                  : action === "export"
                    ? "Получить профиль"
                    : "Подключиться"}
          </button>
        </div>
      </form>
    </Dialog>
  );
}
export function ShareDialog({
  active,
  client,
  onClose,
  onImport,
  onNotice,
}: {
  active: boolean;
  client: ClientExport;
  onClose: () => void;
  onImport: (uri: string) => Promise<boolean>;
  onNotice: (text: string) => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  async function copy() {
    try {
      await navigator.clipboard.writeText(client.uri);
      onNotice("Ссылка профиля скопирована");
    } catch {
      setError("Не удалось скопировать. Выделите ссылку вручную.");
    }
  }
  return (
    <Dialog
      active={active}
      title="Профиль готов"
      onClose={onClose}
      busy={busy}
      className="sh-share-dialog"
    >
      <p className="sh-dialog-intro">
        Отсканируйте QR-код в клиенте с поддержкой Naive HTTPS.
      </p>
      <div className="sh-share-device">
        <span className="sh-icon-box">
          <Smartphone size={23} />
        </span>
        <div>
          <strong>{client.name}</strong>
          <span>Персональный доступ</span>
        </div>
        <CheckCircle2 size={19} />
      </div>
      <div className="sh-qr">
        <QRCodeSVG value={client.uri} size={208} level="M" marginSize={2} />
      </div>
      <div className="sh-share-uri">
        <code>{client.uri}</code>
        <button
          className="icon-button"
          aria-label="Скопировать ссылку профиля"
          onClick={() => void copy()}
        >
          <Copy size={18} />
        </button>
      </div>
      <div className="sh-secret-note">
        <KeyRound size={14} /> QR-код и ссылка дают доступ к вашему VPN.
      </div>
      {error && <InlineError text={error} />}
      <div className="sh-share-actions">
        <button className="button" onClick={() => void copy()}>
          <Copy size={16} /> Скопировать ссылку
        </button>
        <button
          className="button primary"
          disabled={!native || busy}
          onClick={() => {
            setBusy(true);
            void onImport(client.uri)
              .then((ok) => {
                if (ok) onClose();
              })
              .finally(() => setBusy(false));
          }}
        >
          <Plus size={16} /> Добавить на этот ПК
        </button>
      </div>
    </Dialog>
  );
}
function ServerArt() {
  return (
    <div className="sh-server-art" aria-hidden="true">
      <div className="sh-art-glow" />
      <div className="sh-art-grid" />
      <div className="sh-art-orbit a" />
      <div className="sh-art-orbit b" />
      <div className="sh-rack">
        <div className="sh-rack-top">
          <span>COREPASS</span>
          <ShieldCheck size={19} />
        </div>
        {[0, 1, 2].map((i) => (
          <div className="sh-rack-unit" key={i}>
            <div className="sh-rack-vents">
              {[0, 1, 2, 3, 4, 5].map((j) => (
                <i key={j} />
              ))}
            </div>
            <span className="sh-rack-lights">
              <i />
              <i />
            </span>
          </div>
        ))}
        <div className="sh-rack-base" />
      </div>
      <div className="sh-art-label sh-art-label-one">
        <LockKeyhole size={15} />
        <span>Личный сервер</span>
        <i />
      </div>
      <div className="sh-art-label sh-art-label-two">
        <Cable size={15} />
        <span>HTTPS · 443</span>
      </div>
      <div className="sh-art-device sh-art-device-one">
        <Smartphone size={24} />
      </div>
      <div className="sh-art-device sh-art-device-two">
        <Users size={23} />
      </div>
    </div>
  );
}
