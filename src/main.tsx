import React, { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { createRoot } from "react-dom/client";
import {
  Power,
  ShieldCheck,
  ArrowDown,
  ArrowUp,
  Globe2,
  SlidersHorizontal,
  Plus,
  Check,
  Copy,
  RefreshCw,
  ChevronRight,
  ArrowUpRight,
  Router,
  Terminal,
  Activity,
  Layers,
  Settings,
  Search,
  Download,
  X,
  Info,
  Shield,
  CheckCircle2,
  AlertCircle,
  Monitor,
  Wifi,
  Lock,
  ExternalLink,
  Server,
  PanelTop,
} from "lucide-react";
import {
  api,
  bytes,
  duration,
  initial,
  message,
  native,
  type Status,
  type Profile,
  type Event,
} from "./api";
import { SelfHost } from "./SelfHost";
import { SuiPanel } from "./SuiPanel";
import { runOperation } from "./server-api";
import "./style.css";
type Page =
  | "home"
  | "selfhost"
  | "sui"
  | "profiles"
  | "logs"
  | "api"
  | "router"
  | "settings";
const tabs: { id: Page; name: string; icon: typeof Shield }[] = [
  { id: "home", name: "Подключение", icon: Shield },
  { id: "selfhost", name: "Мой сервер", icon: Server },
  { id: "sui", name: "Панель S-UI", icon: PanelTop },
  { id: "profiles", name: "Подключения", icon: Layers },
  { id: "logs", name: "Журнал", icon: Activity },
  { id: "api", name: "Локальный API", icon: Terminal },
  { id: "router", name: "Роутер", icon: Router },
  { id: "settings", name: "Настройки", icon: Settings },
];
const modes = [
  {
    id: "system",
    name: "Браузер и приложения",
    desc: "Через системный прокси Windows",
    icon: Globe2,
  },
  {
    id: "tun",
    name: "Весь компьютер",
    desc: "TUN · нужны права администратора",
    icon: Monitor,
  },
  {
    id: "proxy",
    name: "Локальный прокси",
    desc: "Для ручной настройки приложений",
    icon: SlidersHorizontal,
  },
];
const powershellExample = String.raw`$token = (Get-Content "$env:LOCALAPPDATA\CorepassNext\api-token" -Raw).Trim()
Invoke-RestMethod http://127.0.0.1:17871/v1/status -Headers @{
    Authorization = "Bearer $token"
}`;

function App() {
  const [page, setPage] = useState<Page>(() =>
      !native &&
      ["selfhost", "sui"].includes(
        new URLSearchParams(location.search).get("page") || "",
      )
        ? (new URLSearchParams(location.search).get("page") as Page)
        : "home",
    ),
    [status, setStatus] = useState<Status>(initial),
    [profiles, setProfiles] = useState<Profile[]>([]),
    [events, setEvents] = useState<Event[]>([]),
    [notice, setNotice] = useState(""),
    [error, setError] = useState(""),
    [pending, setPending] = useState(false),
    [serverBusy, setServerBusy] = useState(false),
    [suiBusy, setSuiBusy] = useState(false),
    [modal, setModal] = useState(false),
    [importMode, setImportMode] = useState<"uri" | "url">("url"),
    [input, setInput] = useState(""),
    [filter, setFilter] = useState(""),
    [level, setLevel] = useState("all"),
    [minimize, setMinimize] = useState(true);
  const profile = profiles.find((p) => p.id === status.profile_id),
    connected = ["connected", "degraded"].includes(status.state),
    verified = status.state === "connected",
    busy = pending || status.busy;
  const refresh = async () => {
    const [s, p, l] = await Promise.all([
      api<Status>("GET", "/v1/status"),
      api<{ profiles: Profile[] }>("GET", "/v1/profiles"),
      api<{ events: Event[] }>("GET", "/v1/logs"),
    ]);
    setStatus(s);
    setProfiles(p.profiles);
    setEvents(l.events);
  };
  useEffect(() => {
    let active = true;
    const tick = async () => {
      try {
        if (active) await refresh();
      } catch (e) {
        if (active) setError(message(e));
      }
    };
    void tick();
    const timer = setInterval(tick, 1500);
    void api<{ minimize_to_tray: boolean }>("GET", "/v1/settings")
      .then((v) => setMinimize(v.minimize_to_tray))
      .catch(() => {});
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, []);
  useEffect(() => {
    if (!notice) return;
    const t = setTimeout(() => setNotice(""), 6000);
    return () => clearTimeout(t);
  }, [notice]);
  async function command(path: string, body: object = {}, success = "Готово") {
    setError("");
    setPending(true);
    try {
      const result = await runOperation<{ path?: string }>(path, body);
      setNotice(result.path ? `Отчёт сохранён: ${result.path}` : success);
      await refresh();
      return true;
    } catch (e) {
      setError(message(e));
      return false;
    } finally {
      setPending(false);
    }
  }
  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      setNotice("Скопировано");
    } catch {
      setError("Не удалось скопировать. Выделите текст вручную.");
    }
  }
  function openRepository(e: React.MouseEvent<HTMLAnchorElement>) {
    if (native) {
      e.preventDefault();
      void openUrl("https://github.com/orangepony4/corepass").catch((e) =>
        setError(message(e)),
      );
    }
  }
  const filtered = events.filter(
    (e) =>
      (level === "all" || e.level === level) &&
      `${e.code} ${e.message}`
        .toLocaleLowerCase()
        .includes(filter.toLocaleLowerCase()),
  );
  const chart = status.history;
  const max = Math.max(1, ...chart.map((p) => p.down));
  const points = chart
    .map(
      (p, i) =>
        `${(i * 600) / Math.max(1, chart.length - 1)},${75 - (p.down / max) * 60}`,
    )
    .join(" ");
  return (
    <div className="shell">
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-mark">
            <ShieldCheck size={25} />
          </div>
          <div>
            corepass<span>ВАШ ЛИЧНЫЙ МАРШРУТ</span>
          </div>
        </div>
        <div className="nav-label">РАБОЧЕЕ ПРОСТРАНСТВО</div>
        <nav>
          {tabs.map((t) => (
            <button
              key={t.id}
              className={page === t.id ? "nav active" : "nav"}
              onClick={() => setPage(t.id)}
            >
              <t.icon size={18} />
              {t.name}
              {t.id === "sui" && suiBusy && (
                <RefreshCw
                  size={12}
                  className="sh-spinner nav-operation"
                  aria-label="Запрос к панели выполняется"
                />
              )}
              {t.id === "selfhost" && serverBusy && (
                <RefreshCw
                  size={12}
                  className="sh-spinner nav-operation"
                  aria-label="Операция на сервере выполняется"
                />
              )}
              {page === t.id && <span className="nav-dot" />}
            </button>
          ))}
        </nav>
        <div className="sidebar-bottom">
          <div className="build-card">
            <span className="small-dot" /> Личный VPN
            <span className="version">0.4 preview</span>
          </div>
          <p>
            Приватный доступ.
            <br />
            Понятное управление.
          </p>
          <div className="local-label">
            <Lock size={12} /> Без облачной панели
          </div>
        </div>
      </aside>
      <main>
        <header>
          <div className="breadcrumb">
            Corepass <ChevronRight size={13} />
            <span>{tabs.find((t) => t.id === page)?.name}</span>
          </div>
          <div className="header-right">
            <span className="platform">
              <Monitor size={14} /> Windows
            </span>
            <span className="avatar">C</span>
          </div>
        </header>
        <div className="content">
          <div className="page-heading page-transition" key={page}>
            <div>
              <div className="eyebrow">ВАШ ИНТЕРНЕТ. ВАШИ ПРАВИЛА.</div>
              <h1>{tabs.find((t) => t.id === page)?.name}</h1>
              <p>
                {
                  {
                    home: "Надёжное соединение начинается с понятных деталей.",
                    selfhost:
                      "Настройте свой VPN и управляйте доступом устройств.",
                    sui: "Клиенты и профили вашей установленной S-UI.",
                    profiles: "Профили и подписки для подключения к VPN.",
                    logs: "Что происходит с подключением — без догадок.",
                    api: "Управляйте Corepass из скриптов и других приложений.",
                    router: "Тот же сервер — для устройств во всём доме.",
                    settings: "Настройте подключение под свой сценарий.",
                  }[page]
                }
              </p>
            </div>
            <span className="release">
              NEXT <span>0.4 PREVIEW</span>
            </span>
          </div>
          {!native && (
            <div className="banner neutral">
              <Info size={20} />
              <div>
                <strong>Предпросмотр интерфейса</strong>
                <p>
                  Управление VPN доступно в настольном приложении. Показатели
                  подключения здесь не собираются.
                </p>
              </div>
            </div>
          )}
          {status.legacy_active && (
            <div className="banner">
              <ShieldCheck size={21} />
              <div>
                <strong>Обнаружена старая версия Corepass</strong>
                <p>
                  Обнаружен Corepass 0.2. Новый клиент работает отдельно;
                  подключение заблокировано до выхода из старой версии.
                </p>
              </div>
            </div>
          )}
          {error && (
            <div className="alert" role="alert">
              <AlertCircle size={18} />
              <span>{error}</span>
              <button
                aria-label="Закрыть сообщение"
                onClick={() => setError("")}
              >
                <X size={16} />
              </button>
            </div>
          )}
          <div hidden={page !== "selfhost"}>
            <SelfHost
              active={page === "selfhost"}
              onImport={(uri) =>
                command(
                  "/v1/profiles/import",
                  { uri },
                  "Профиль добавлен в подключения",
                )
              }
              onNotice={setNotice}
              onBusyChange={setServerBusy}
            />
          </div>
          <div hidden={page !== "sui"}>
            <SuiPanel
              active={page === "sui"}
              onImport={(uri) =>
                command(
                  "/v1/profiles/import",
                  { uri },
                  "Профиль добавлен в подключения",
                )
              }
              onNotice={setNotice}
              onBusyChange={setSuiBusy}
            />
          </div>
          {page === "home" && (
            <>
              <div className="home-grid">
                <section className="connection card">
                  <div className="card-top">
                    <span className="section-label">
                      ЗАЩИЩЁННОЕ ПОДКЛЮЧЕНИЕ
                    </span>
                    <span className={`pill ${verified ? "good" : ""}`}>
                      <i />
                      {status.state === "degraded"
                        ? "Нет ответа"
                        : connected
                          ? "Подключено"
                          : status.state === "connecting"
                            ? "Проверяем маршрут"
                            : "Не подключено"}
                    </span>
                  </div>
                  <div
                    className={`power-stage ${verified ? "on" : ""} ${status.state === "connecting" ? "is-connecting" : ""}`}
                  >
                    <div className="orbit one" />
                    <div className="orbit two" />
                    <div className="orbit-dot" />
                    <button
                      className="power"
                      aria-label={
                        connected ? "Отключить VPN" : "Подключить VPN"
                      }
                      disabled={
                        !native ||
                        busy ||
                        (!connected &&
                          (status.legacy_active ||
                            !profile ||
                            !status.engine_available))
                      }
                      onClick={() =>
                        void command(
                          connected ? "/v1/disconnect" : "/v1/connect",
                          {},
                          connected
                            ? "Соединение отключено"
                            : "Соединение проверено",
                        )
                      }
                    >
                      <Power size={42} strokeWidth={1.6} />
                    </button>
                  </div>
                  <h2>
                    {status.state === "degraded"
                      ? "Маршрут не отвечает"
                      : connected
                        ? "Вы на связи"
                        : status.state === "connecting"
                          ? "Устанавливаем соединение"
                          : status.legacy_active
                            ? "Готов, когда понадоблюсь"
                            : "Один шаг до подключения"}
                  </h2>
                  <p className="connection-message">
                    {status.legacy_active
                      ? "Новый клиент не изменяет настройки старого."
                      : status.message}
                  </p>
                  <div className="connection-footer">
                    <div>
                      <span>Время подключения</span>
                      <strong>{duration(status.uptime_seconds)}</strong>
                    </div>
                    <div>
                      <span>Задержка HTTPS</span>
                      <strong>
                        {status.latency_ms == null
                          ? "—"
                          : `${status.latency_ms} мс`}
                      </strong>
                    </div>
                  </div>
                  {busy && (
                    <button
                      className="cancel"
                      onClick={() =>
                        void api("POST", "/v1/cancel").catch((e) =>
                          setError(message(e)),
                        )
                      }
                    >
                      Отменить операцию
                    </button>
                  )}
                  {connected && !busy && (
                    <button
                      className="cancel"
                      onClick={() =>
                        void command(
                          "/v1/diagnostics",
                          {},
                          "Проверка завершена. Результаты сохранены в журнале.",
                        )
                      }
                    >
                      Проверить соединение
                    </button>
                  )}
                </section>
                <section className="server-panel card">
                  <div className="card-top">
                    <span className="section-label">ВАШ МАРШРУТ</span>
                    <Globe2 size={17} />
                  </div>
                  <RouteArt connected={verified} />
                  <div className="server-title">
                    <div className="server-symbol">
                      <Globe2 size={25} />
                    </div>
                    <div>
                      <h3>{profile?.name || "Добавьте свой сервер"}</h3>
                      <p>
                        {profile
                          ? `${profile.server}:${profile.port}`
                          : "Импортируйте ссылку или подписку"}
                      </p>
                    </div>
                  </div>
                  <div className="route-line">
                    <div>
                      <span className="route-dot local" />
                      <span>Это устройство</span>
                    </div>
                    <span className="route-dashes" />
                    <div>
                      <span
                        className={`route-dot ${connected ? "good" : ""}`}
                      />
                      <span>
                        {profile ? "Личный сервер" : "Сервер не выбран"}
                      </span>
                    </div>
                  </div>
                  <button
                    className="wide-button"
                    onClick={() => setPage("profiles")}
                  >
                    {profile ? "Выбрать сервер" : "Добавить сервер"}
                    <ChevronRight size={17} />
                  </button>
                  <div className="exit-ip">
                    <span>Внешний IP через VPN</span>
                    <strong>{status.exit_ip || "—"}</strong>
                  </div>
                </section>
              </div>
              <div className="stats-grid">
                <div className="stat card">
                  <div className="stat-label">
                    <ArrowDown size={16} />
                    ПОЛУЧЕНО
                  </div>
                  <strong>{bytes(status.download_bytes)}</strong>
                  <span>{bytes(status.download_rate)}/с сейчас</span>
                </div>
                <div className="stat card">
                  <div className="stat-label">
                    <ArrowUp size={16} />
                    ОТПРАВЛЕНО
                  </div>
                  <strong>{bytes(status.upload_bytes)}</strong>
                  <span>{bytes(status.upload_rate)}/с сейчас</span>
                </div>
                <div className="traffic card">
                  <div className="card-top">
                    <span className="section-label">СКОРОСТЬ ПОЛУЧЕНИЯ</span>
                    <span className="muted">За текущий сеанс</span>
                  </div>
                  <svg
                    viewBox="0 0 600 84"
                    preserveAspectRatio="none"
                    aria-label="График реальной скорости"
                  >
                    <path
                      d="M0 75H600 M0 40H600 M0 5H600"
                      stroke="#242c35"
                      strokeDasharray="3 5"
                      fill="none"
                    />
                    <polyline
                      points={points || "0,75 600,75"}
                      fill="none"
                      stroke="#b9ee85"
                      strokeWidth="2.5"
                      vectorEffect="non-scaling-stroke"
                    />
                  </svg>
                </div>
              </div>
              <section className="mode-section">
                <div className="section-title">
                  <h3>Как подключаться</h3>
                  <span>Можно изменить перед подключением</span>
                </div>
                <div className="modes">
                  {modes.map((m) => (
                    <button
                      key={m.id}
                      className={`mode ${status.mode === m.id ? "selected" : ""}`}
                      disabled={!native || busy || connected}
                      onClick={() =>
                        void command(
                          "/v1/settings",
                          { mode: m.id },
                          "Режим сохранён",
                        )
                      }
                    >
                      <m.icon size={21} />
                      <div>
                        <strong>{m.name}</strong>
                        <span>{m.desc}</span>
                      </div>
                      <span className="radio">
                        {status.mode === m.id && <i />}
                      </span>
                    </button>
                  ))}
                </div>
              </section>
            </>
          )}
          {page === "profiles" && (
            <>
              <div className="toolbar">
                <div className="count">Серверы: {profiles.length}</div>
                <div className="button-group">
                  <button
                    className="button"
                    disabled={!native || busy || connected}
                    onClick={() =>
                      void command(
                        "/v1/profiles/migrate",
                        {},
                        "Профили скопированы. Старое приложение продолжает работать.",
                      )
                    }
                  >
                    <Copy size={16} />
                    Из старого Corepass
                  </button>
                  <button
                    className="button"
                    disabled={!native || busy || connected}
                    onClick={() =>
                      void command(
                        "/v1/profiles/refresh",
                        {},
                        "Подписка обновлена",
                      )
                    }
                  >
                    <RefreshCw size={16} />
                    Обновить
                  </button>
                  <button
                    className="button primary"
                    disabled={busy || connected}
                    onClick={() => setModal(true)}
                  >
                    <Plus size={17} />
                    Добавить
                  </button>
                </div>
              </div>
              {!profiles.length ? (
                <div className="empty card">
                  <div className="empty-icon">
                    <Layers size={35} />
                  </div>
                  <h2>Ваши серверы будут здесь</h2>
                  <p>
                    Добавьте Naive HTTPS ссылку, подписку S-UI
                    <br />
                    или скопируйте профили из старого клиента.
                  </p>
                  <button
                    className="button primary"
                    onClick={() => setModal(true)}
                  >
                    <Plus size={17} />
                    Добавить первый сервер
                  </button>
                </div>
              ) : (
                <div className="profile-list">
                  {profiles.map((p) => (
                    <div
                      className={`profile card ${p.selected ? "chosen" : ""}`}
                      key={p.id}
                    >
                      <div className="server-symbol">
                        <Globe2 size={25} />
                      </div>
                      <div className="profile-info">
                        <h3>{p.name}</h3>
                        <p>
                          {p.server}:{p.port}
                        </p>
                      </div>
                      <span className="protocol">Naive / HTTPS</span>
                      <button
                        className={p.selected ? "button quiet" : "button"}
                        disabled={!native || p.selected || busy || connected}
                        onClick={() =>
                          void command(
                            "/v1/profiles/select",
                            { profile_id: p.id },
                            "Сервер выбран",
                          )
                        }
                      >
                        {p.selected ? (
                          <>
                            <Check size={16} />
                            Выбран
                          </>
                        ) : (
                          "Выбрать"
                        )}
                      </button>
                    </div>
                  ))}
                </div>
              )}
              <div className="note">
                <Lock size={15} />
                Пароли хранятся в папке пользователя с ограниченным доступом и
                исключаются из журналов.
              </div>
            </>
          )}
          {page === "logs" && (
            <>
              <div className="toolbar">
                <div className="search">
                  <Search size={16} />
                  <input
                    aria-label="Поиск в журнале"
                    placeholder="Найти событие или код ошибки"
                    value={filter}
                    onChange={(e) => setFilter(e.target.value)}
                  />
                </div>
                <div className="button-group">
                  <select
                    aria-label="Уровень журнала"
                    value={level}
                    onChange={(e) => setLevel(e.target.value)}
                  >
                    <option value="all">Все события</option>
                    <option value="error">Ошибки</option>
                    <option value="warn">Предупреждения</option>
                    <option value="info">Информация</option>
                  </select>
                  <button
                    className="button"
                    disabled={!native || busy}
                    onClick={() => void command("/v1/support/export")}
                  >
                    <Download size={16} />
                    Сохранить отчёт
                  </button>
                </div>
              </div>
              <section className="log-panel card">
                <div className="log-heading">
                  <span>ВРЕМЯ</span>
                  <span>СОБЫТИЕ</span>
                  <span>{filtered.length} записей</span>
                </div>
                {!filtered.length ? (
                  <div className="log-empty">
                    <CheckCircle2 size={25} />
                    <p>
                      {filter || level !== "all"
                        ? "Нет событий по этому фильтру"
                        : "Журнал пока пуст"}
                    </p>
                  </div>
                ) : (
                  filtered.map((e, i) => (
                    <div
                      className={`log-row ${e.level}`}
                      key={`${e.time}-${i}`}
                    >
                      <time>
                        {new Date(e.time).toLocaleTimeString("ru-RU")}
                      </time>
                      <div>
                        <span className="event-code">{e.code}</span>
                        <p>{e.message}</p>
                      </div>
                      <span className="log-level">
                        {e.level === "error"
                          ? "Ошибка"
                          : e.level === "warn"
                            ? "Внимание"
                            : "Инфо"}
                      </span>
                    </div>
                  ))
                )}
              </section>
              <div className="note">
                <Info size={15} />
                Журнал сохраняется между запусками. Токены, пароли и адреса
                подписок скрываются автоматически.
              </div>
            </>
          )}
          {page === "api" && (
            <>
              <div className="info-grid">
                <section className="card padded">
                  <div className="card-top">
                    <span className="section-label">ЛОКАЛЬНОЕ УПРАВЛЕНИЕ</span>
                    <Terminal size={19} />
                  </div>
                  <h2>
                    Открыт для автоматизации.
                    <br />
                    Доступен только вам.
                  </h2>
                  <p className="body-text">
                    API слушает только 127.0.0.1. Для команд нужен Bearer-токен
                    из защищённой папки приложения.
                  </p>
                  <label className="field-label">АДРЕС API</label>
                  <div className="copy-field">
                    <code>{status.api_url}</code>
                    <button
                      aria-label="Копировать адрес API"
                      onClick={() => void copy(status.api_url)}
                    >
                      <Copy size={16} />
                    </button>
                  </div>
                  <label className="field-label">ФАЙЛ ТОКЕНА</label>
                  <div className="copy-field">
                    <code>{status.token_path}</code>
                    <button
                      aria-label="Копировать путь токена"
                      onClick={() => void copy(status.token_path)}
                    >
                      <Copy size={16} />
                    </button>
                  </div>
                </section>
                <section className="card padded">
                  <span className="section-label">ОСНОВНЫЕ КОМАНДЫ</span>
                  <div className="endpoints">
                    {[
                      ["GET", "/v1/status", "Состояние и реальная статистика"],
                      ["GET", "/v1/logs", "Постоянный журнал"],
                      ["POST", "/v1/connect", "Проверенное подключение"],
                      ["POST", "/v1/disconnect", "Отключение новой версии"],
                      ["POST", "/v1/profiles/subscribe", "Добавить подписку"],
                      ["GET", "/v1/openapi.json", "Полная схема API"],
                    ].map(([m, p, d]) => (
                      <div key={p}>
                        <span
                          className={m === "GET" ? "http-get" : "http-post"}
                        >
                          {m}
                        </span>
                        <code>{p}</code>
                        <small>{d}</small>
                      </div>
                    ))}
                  </div>
                </section>
              </div>
              <div className="card padded code-card">
                <div className="section-title">
                  <h3>Пример для PowerShell</h3>
                  <button
                    className="button"
                    onClick={() => void copy(powershellExample)}
                  >
                    <Copy size={15} />
                    Скопировать
                  </button>
                </div>
                <pre>{powershellExample}</pre>
              </div>
            </>
          )}
          {page === "router" && (
            <>
              <section className="card router-hero">
                <div>
                  <span className="eyebrow">COREPASS ДЛЯ OPENWRT</span>
                  <h2>
                    Один сервер.
                    <br />
                    Весь дом на связи.
                  </h2>
                  <p>
                    Лёгкая панель в LuCI для Cudy WR3000S v1.
                    <br />
                    Подключение по устройствам рядом со Steer и Zapret Manager.
                  </p>
                  <a
                    className="button primary"
                    href="https://github.com/orangepony4/corepass"
                    onClick={openRepository}
                    target="_blank"
                    rel="noreferrer"
                  >
                    Исходный код и установка
                    <ArrowUpRight size={17} />
                  </a>
                </div>
                <div className="router-art">
                  <Wifi size={80} strokeWidth={1} />
                  <Router size={100} strokeWidth={1} />
                </div>
              </section>
              <div className="three-cards">
                <div className="card padded">
                  <ShieldCheck />
                  <h3>Установка без переключения сети</h3>
                  <p>
                    Модуль устанавливается отключённым. Текущие туннели
                    продолжают работать.
                  </p>
                </div>
                <div className="card padded">
                  <Monitor />
                  <h3>Начните с одного устройства</h3>
                  <p>
                    Выберите MAC тестового устройства. Остальная домашняя сеть
                    сохранит прежний маршрут.
                  </p>
                </div>
                <div className="card padded">
                  <Activity />
                  <h3>Состояние и диагностика</h3>
                  <p>
                    Проверка конфигурации, реального выхода в интернет и журнал
                    службы доступны в LuCI.
                  </p>
                </div>
              </div>
            </>
          )}
          {page === "settings" && (
            <section className="card padded settings-card">
              <span className="section-label">ПОВЕДЕНИЕ ПРИЛОЖЕНИЯ</span>
              <div className="setting-row">
                <div>
                  <h3>Сворачивать в трей при закрытии</h3>
                  <p>
                    Подключение продолжит работать. Полный выход доступен в меню
                    значка рядом с часами.
                  </p>
                </div>
                <button
                  role="switch"
                  aria-checked={minimize}
                  aria-label="Сворачивать в трей"
                  disabled={!native || busy || connected}
                  className={`switch ${minimize ? "checked" : ""}`}
                  onClick={() =>
                    void command(
                      "/v1/settings",
                      { minimize_to_tray: !minimize },
                      "Настройка сохранена",
                    ).then((ok) => ok && setMinimize(!minimize))
                  }
                >
                  <i />
                </button>
              </div>
              <div className="setting-row">
                <div>
                  <h3>Хранение данных</h3>
                  <p>{status.data_dir}</p>
                </div>
                <Lock size={20} />
              </div>
              <div className="setting-row">
                <div>
                  <h3>Движок подключения</h3>
                  <p>sing-box · отдельный процесс под управлением Rust</p>
                </div>
                <span
                  className={`pill ${status.engine_available ? "good" : ""}`}
                >
                  {status.engine_available ? "Готов" : "Не найден"}
                </span>
              </div>
              <div className="setting-row">
                <div>
                  <h3>Версия приложения</h3>
                  <p>Corepass Next {status.version} · Rust / Tauri 2</p>
                </div>
                <a
                  href="https://github.com/orangepony4/corepass"
                  onClick={openRepository}
                  target="_blank"
                  rel="noreferrer"
                  aria-label="Открыть GitHub"
                >
                  <ExternalLink size={19} />
                </a>
              </div>
            </section>
          )}
          <footer>
            <div>
              <Lock size={12} />
              Локальное управление · без телеметрии
            </div>
            <span>COREPASS NEXT / 0.4 PREVIEW</span>
          </footer>
        </div>
      </main>
      {notice && (
        <div className="toast" role="status">
          <CheckCircle2 size={18} />
          {notice}
          <button
            aria-label="Закрыть уведомление"
            onClick={() => setNotice("")}
          >
            <X size={15} />
          </button>
        </div>
      )}
      {modal && (
        <div
          className="modal-backdrop"
          onClick={() => !busy && setModal(false)}
        >
          <section
            className="modal card"
            role="dialog"
            aria-modal="true"
            aria-labelledby="import-title"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="section-title">
              <h2 id="import-title">Добавить сервер</h2>
              <button
                className="icon-button"
                disabled={busy}
                aria-label="Закрыть импорт"
                onClick={() => setModal(false)}
              >
                <X size={21} />
              </button>
            </div>
            <p>Используйте персональную ссылку из вашей панели S-UI.</p>
            <div className="segments">
              <button
                className={importMode === "url" ? "active" : ""}
                onClick={() => setImportMode("url")}
              >
                Подписка HTTPS
              </button>
              <button
                className={importMode === "uri" ? "active" : ""}
                onClick={() => setImportMode("uri")}
              >
                Ссылка Naive
              </button>
            </div>
            <label className="field-label" htmlFor="import-value">
              {importMode === "url" ? "ССЫЛКА ПОДПИСКИ" : "ССЫЛКА ПОДКЛЮЧЕНИЯ"}
            </label>
            <textarea
              id="import-value"
              value={input}
              onChange={(e) => setInput(e.target.value)}
              placeholder={
                importMode === "url"
                  ? "https://ваш-домен/подписка?format=json"
                  : "naive+https://логин:пароль@домен:443"
              }
              autoComplete="off"
              spellCheck={false}
            />
            <div className="note">
              <Lock size={15} />
              Ссылка хранится локально.
            </div>
            <div className="modal-actions">
              <button
                className="button"
                disabled={busy}
                onClick={() => setModal(false)}
              >
                Отмена
              </button>
              <button
                className="button primary"
                disabled={!native || busy || !input.trim()}
                onClick={() =>
                  void command(
                    importMode === "url"
                      ? "/v1/profiles/subscribe"
                      : "/v1/profiles/import",
                    { [importMode]: input.trim() },
                    "Сервер добавлен",
                  ).then((ok) => {
                    if (ok) {
                      setModal(false);
                      setInput("");
                    }
                  })
                }
              >
                {busy ? "Проверяем…" : "Добавить сервер"}
                <ChevronRight size={16} />
              </button>
            </div>
          </section>
        </div>
      )}
    </div>
  );
}
function RouteArt({ connected }: { connected: boolean }) {
  return (
    <div className={`map-art ${connected ? "active" : ""}`}>
      <svg viewBox="0 0 400 150" aria-hidden="true">
        <defs>
          <pattern
            id="dots"
            width="12"
            height="12"
            patternUnits="userSpaceOnUse"
          >
            <circle cx="2" cy="2" r="1" fill="#38454d" />
          </pattern>
          <linearGradient id="route">
            <stop stopColor="#52675c" />
            <stop offset="1" stopColor={connected ? "#b9ee85" : "#829b70"} />
          </linearGradient>
        </defs>
        <path
          d="M35 65 62 29 115 25 145 44 170 40 192 53 210 32 270 26 290 50 335 42 367 75 350 92 319 87 280 112 245 104 225 125 193 108 159 120 135 91 110 83 85 105 55 93Z"
          fill="url(#dots)"
        />
        <path
          d="M107 90 Q190 6 282 65"
          fill="none"
          stroke="url(#route)"
          strokeWidth="1.7"
          strokeDasharray="4 5"
        />
        <circle cx="107" cy="90" r="4" fill="#8daca0" />
        <circle cx="282" cy="65" r="14" fill="#b9ee85" opacity=".08" />
        <circle cx="282" cy="65" r="7" fill="#b9ee85" opacity=".18" />
        <circle cx="282" cy="65" r="3" fill="#b9ee85" />
      </svg>
    </div>
  );
}

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
