import { useEffect, useState, type FormEvent } from "react";
import {
  AlertCircle,
  ArrowRight,
  Cable,
  Check,
  CircleHelp,
  Eye,
  EyeOff,
  Globe2,
  KeyRound,
  Layers3,
  LoaderCircle,
  LockKeyhole,
  Plus,
  QrCode,
  RefreshCw,
  Settings2,
  ShieldCheck,
  Smartphone,
  UserRound,
  Users,
  Workflow,
  X,
} from "lucide-react";
import { api, bytes, message, native } from "./api";
import { Dialog, ShareDialog } from "./SelfHost";
import { runOperation } from "./server-api";
import {
  clientsForInbound,
  mayCreateClient,
  suiConfigError,
  performSuiAction,
  type SuiClient,
  type SuiClientResult,
  type SuiConfig,
  type SuiInbound,
  type SuiPanelInfo,
  type SuiResult,
} from "./sui-api";
import "./sui.css";

const preview =
  !native && new URLSearchParams(location.search).get("page") === "sui"
    ? new URLSearchParams(location.search).get("preview")
    : null;
const demoPanel: SuiPanelInfo = {
  id: "preview-sui",
  name: "Моя Финляндия",
  panel_host: "panel.example.com",
  server_domain: "vpn.example.com",
  server_port: 443,
  read_only: preview !== "create",
  credential_saved: preview !== "token",
  created_at: "2026-10-21T12:00:00Z",
};
const demoInbounds: SuiInbound[] = [
  {
    id: 1,
    tag: "Finland HTTPS",
    type: "naive",
    listen_port: 1443,
    client_count: 3,
  },
];
const demoClients: SuiClient[] = [
  {
    id: 1,
    name: "Мой компьютер",
    enable: true,
    inbounds: [1],
    expiry: 0,
    volume: 0,
    up: 1288490188,
    down: 5561982976,
  },
  {
    id: 2,
    name: "iPhone",
    enable: true,
    inbounds: [1],
    expiry: 0,
    volume: 0,
    up: 193273528,
    down: 1245540515,
  },
  {
    id: 3,
    name: "Планшет",
    enable: false,
    inbounds: [1],
    expiry: 0,
    volume: 0,
    up: 0,
    down: 0,
  },
];
const demoShare: SuiClientResult = {
  id: 2,
  name: "iPhone",
  uri: "naive+https://preview:example-only@vpn.example.com:443#iPhone",
};
const blankConfig = (): SuiConfig => ({
  name: "",
  base_url: "",
  server_domain: "",
  server_port: 443,
  read_only: true,
});

export function SuiPanel({
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
  const hasFixture = Boolean(preview && preview !== "settings");
  const [panels, setPanels] = useState<SuiPanelInfo[]>(
    hasFixture ? [demoPanel] : [],
  );
  const [selected, setSelected] = useState<SuiPanelInfo | null>(
    hasFixture ? demoPanel : null,
  );
  const [config, setConfig] = useState<{
    value: SuiConfig;
    id?: string;
  } | null>(
    preview === "settings" || preview === "vault"
      ? {
          ...(preview === "vault" ? { id: demoPanel.id } : {}),
          value: {
            ...blankConfig(),
            name: "Моя Финляндия",
            base_url:
              preview === "vault"
                ? ""
                : "https://panel.example.com/your-panel-path/",
            server_domain: "vpn.example.com",
          },
        }
      : null,
  );
  const [inbounds, setInbounds] = useState<SuiInbound[]>(
    hasFixture ? demoInbounds : [],
  );
  const [inbound, setInbound] = useState<number | null>(hasFixture ? 1 : null);
  const [clients, setClients] = useState<SuiClient[]>(
    hasFixture ? demoClients : [],
  );
  const [listed, setListed] = useState(hasFixture);
  const [loading, setLoading] = useState(native);
  const [error, setError] = useState("");
  const [quickBusy, setQuickBusy] = useState(false);
  const [failedAction, setFailedAction] = useState<{
    action: "inspect" | "list" | "export";
    client?: SuiClient;
  } | null>(null);
  const [access, setAccess] = useState<{
    action: "inspect" | "list" | "create" | "export";
    client?: SuiClient;
    forceToken?: boolean;
  } | null>(
    preview === "create"
      ? { action: "create" }
      : preview === "token"
        ? { action: "list" }
        : null,
  );
  const [share, setShare] = useState<SuiClientResult | null>(
    preview === "share" ? demoShare : null,
  );

  async function refreshPanels() {
    if (!native) return;
    setLoading(true);
    try {
      const value = await api<{ panels: SuiPanelInfo[] }>(
        "GET",
        "/v1/sui/panels",
      );
      setPanels(value.panels);
      setSelected((previous) =>
        previous
          ? (value.panels.find((panel) => panel.id === previous.id) ?? null)
          : previous,
      );
    } catch (e) {
      setError(message(e));
    } finally {
      setLoading(false);
    }
  }
  useEffect(() => {
    void refreshPanels();
  }, []);
  useEffect(() => {
    onBusyChange(quickBusy);
    return () => onBusyChange(false);
  }, [quickBusy, onBusyChange]);
  function credentialChanged(panelId: string, saved: boolean) {
    setPanels((previous) =>
      previous.map((panel) =>
        panel.id === panelId ? { ...panel, credential_saved: saved } : panel,
      ),
    );
    setSelected((previous) =>
      previous?.id === panelId
        ? { ...previous, credential_saved: saved }
        : previous,
    );
  }
  async function openAction(
    action: "inspect" | "list" | "create" | "export",
    client?: SuiClient,
  ) {
    if (!selected || quickBusy) return;
    setError("");
    setFailedAction(null);
    if (!selected.credential_saved || action === "create") {
      setAccess({ action, client });
      return;
    }
    setQuickBusy(true);
    try {
      const result = await performSuiAction({
        panelId: selected.id,
        action,
        stored: true,
        inbound,
        clientId: client?.id,
      });
      receive(result, action);
    } catch (e) {
      setError(message(e));
      setFailedAction({ action, client });
      void refreshPanels();
    } finally {
      setQuickBusy(false);
    }
  }
  function selectPanel(panel: SuiPanelInfo) {
    setSelected(panel);
    setInbounds([]);
    setInbound(null);
    setClients([]);
    setListed(false);
    setError("");
    setFailedAction(null);
  }
  const visible = clientsForInbound(clients, inbound);
  function receive(value: SuiResult, action = access?.action) {
    if (value.inbounds) {
      setInbounds(value.inbounds);
      setInbound((previous) =>
        value.inbounds!.some((item) => item.id === previous)
          ? previous
          : (value.inbounds![0]?.id ?? null),
      );
    }
    if (value.clients) {
      setClients(value.clients);
      setListed(true);
    }
    if (value.client) {
      if (action === "export") setShare(value.client);
      if (action === "create" && inbound !== null)
        setClients((previous) => [
          ...previous,
          {
            id: value.client!.id,
            name: value.client!.name,
            enable: true,
            inbounds: [inbound],
            expiry: 0,
            volume: 0,
            up: 0,
            down: 0,
          },
        ]);
    }
    setAccess(null);
    onNotice(
      action === "create"
        ? "Клиент создан. Текущие соединения сервера могли переподключиться."
        : action === "inspect"
          ? value.inbounds?.length
            ? "Панель ответила. Найдены входящие подключения Naive."
            : "Панель ответила. Подключения Naive не найдены."
          : action === "list"
            ? "Клиенты загружены из панели"
            : "Профиль получен из S-UI",
    );
  }

  return (
    <div className="sui-page">
      {preview && (
        <div className="sui-preview-note" role="status">
          <CircleHelp size={14} /> Демонстрационные данные. Запросы к панели в
          предпросмотре отключены.
        </div>
      )}
      {error && (
        <div className="sh-error" role="alert">
          <AlertCircle size={18} />
          <p>{error}</p>
          {failedAction && (
            <button
              className="button sui-token-retry"
              onClick={() => {
                setAccess({ ...failedAction, forceToken: true });
                setError("");
                setFailedAction(null);
              }}
            >
              Ввести другой токен
            </button>
          )}
        </div>
      )}
      {quickBusy && (
        <div className="sui-request-progress" role="status">
          <LoaderCircle size={16} className="sh-spinner" /> Запрос к панели
          выполняется…
        </div>
      )}
      {panels.length === 0 && !loading ? (
        <>
          <section className="card sui-welcome">
            <div>
              <span className="eyebrow">
                <span className="sh-mini-dot" /> СУЩЕСТВУЮЩИЙ VPN · S-UI
              </span>
              <h2>
                Ваша панель.
                <br />
                <span>Теперь в Corepass.</span>
              </h2>
              <p>
                Подключите S-UI, которая уже работает на сервере.
                <br />
                Смотрите клиентов и выдавайте профили с QR-кодом
                <br />
                из привычного приложения.
              </p>
              <button
                className="button primary"
                onClick={() => setConfig({ value: blankConfig() })}
              >
                <Plus size={17} /> Добавить панель <ArrowRight size={16} />
              </button>
              <span className="sui-welcome-note">
                <LockKeyhole size={13} /> Токен можно защитить хранилищем
                Windows
              </span>
            </div>
            <div className="sui-connection-art" aria-hidden="true">
              <span className="sui-art-end">
                <ShieldCheck size={29} />
                <small>COREPASS</small>
              </span>
              <span className="sui-art-track">
                <i />
                <i />
                <i />
              </span>
              <span className="sui-art-end panel">
                <Layers3 size={32} />
                <small>S-UI</small>
              </span>
              <span className="sui-art-caption">
                <Cable size={13} /> Прямое управление через API
              </span>
            </div>
          </section>
          <div className="sui-feature-grid">
            {[
              {
                icon: Users,
                title: "Клиенты под рукой",
                text: "Список устройств, состояние доступа и переданный трафик из вашей панели.",
              },
              {
                icon: QrCode,
                title: "Профиль одним сканированием",
                text: "Отдельная ссылка и QR-код для выбранного клиента Naive HTTPS.",
              },
              {
                icon: ShieldCheck,
                title: "Сначала только чтение",
                text: "Просмотр и экспорт доступны без изменения конфигурации сервера.",
              },
            ].map((item) => (
              <article className="card sui-feature" key={item.title}>
                <span className="sh-icon-box">
                  <item.icon size={21} />
                </span>
                <h3>{item.title}</h3>
                <p>{item.text}</p>
              </article>
            ))}
          </div>
          <div className="sui-bottom-note">
            <CircleHelp size={17} />
            <p>
              Это подключение к вашей установленной S-UI. Для чистого VPS
              используйте раздел <strong>«Мой сервер»</strong>.
            </p>
            <span className="sh-beta">PREVIEW</span>
          </div>
        </>
      ) : (
        <>
          <div className="sui-toolbar">
            <div>
              <h3>Ваши панели</h3>
              <p>Прямой доступ к серверу · без облачного посредника</p>
            </div>
            <button
              className="button primary"
              onClick={() => setConfig({ value: blankConfig() })}
            >
              <Plus size={16} /> Добавить панель
            </button>
          </div>
          <div className="sui-layout">
            <aside className="sui-panel-list">
              {loading && !panels.length && (
                <div className="sh-loading">
                  <LoaderCircle size={18} className="sh-spinner" /> Загружаем
                  панели…
                </div>
              )}
              {panels.map((panel) => (
                <button
                  key={panel.id}
                  disabled={quickBusy}
                  className={`card sui-panel-option ${selected?.id === panel.id ? "selected" : ""}`}
                  onClick={() => selectPanel(panel)}
                >
                  <span className="sh-icon-box">
                    <Layers3 size={20} />
                  </span>
                  <div>
                    <strong>{panel.name}</strong>
                    <span>{panel.panel_host}</span>
                    <small>
                      {panel.read_only
                        ? "Только чтение"
                        : "Создание клиентов разрешено"}
                    </small>
                  </div>
                  {selected?.id === panel.id && <Check size={15} />}
                </button>
              ))}
              <div className="sh-local-note">
                <LockKeyhole size={14} /> Настройки хранятся на этом ПК.
              </div>
            </aside>
            {selected ? (
              <section className="card sui-clients">
                <div className="sui-panel-heading">
                  <div>
                    <span className="eyebrow">ВАША ПАНЕЛЬ S-UI</span>
                    <h2>{selected.name}</h2>
                    <p>
                      <Globe2 size={13} />
                      {selected.panel_host}
                    </p>
                  </div>
                  <button
                    className="icon-button"
                    aria-label="Настройки панели S-UI"
                    disabled={quickBusy}
                    onClick={() =>
                      setConfig({
                        id: selected.id,
                        value: {
                          name: selected.name,
                          base_url: "",
                          server_domain: selected.server_domain,
                          server_port: selected.server_port,
                          read_only: selected.read_only,
                        },
                      })
                    }
                  >
                    <Settings2 size={18} />
                  </button>
                </div>
                <div className="sui-endpoint-line">
                  <LockKeyhole size={15} />
                  <div>
                    <strong>
                      {selected.server_domain}:{selected.server_port}
                    </strong>
                    <span>Публичный адрес в профилях клиентов</span>
                  </div>
                  <span
                    className={`sui-access-badge ${selected.read_only ? "" : "write"}`}
                  >
                    {selected.read_only ? "Только чтение" : "Можно создавать"}
                  </span>
                </div>
                {selected.credential_saved && (
                  <div className="sui-vault-inline">
                    <ShieldCheck size={14} /> Токен сохранён в Windows
                  </div>
                )}
                <div className="sui-client-controls">
                  <div>
                    <label htmlFor="sui-inbound">Подключение Naive</label>
                    <select
                      id="sui-inbound"
                      value={inbound ?? ""}
                      disabled={!inbounds.length}
                      onChange={(event) =>
                        setInbound(Number(event.target.value))
                      }
                    >
                      {!inbounds.length && (
                        <option value="">Загрузите данные панели</option>
                      )}
                      {inbounds.map((item) => (
                        <option value={item.id} key={item.id}>
                          {item.tag || `Naive ${item.id}`}
                        </option>
                      ))}
                    </select>
                  </div>
                  <button
                    className="icon-button"
                    aria-label="Загрузить клиентов S-UI"
                    disabled={!native || quickBusy}
                    onClick={() => void openAction("list")}
                  >
                    <RefreshCw size={17} />
                  </button>
                  <button
                    className="button primary"
                    disabled={
                      !native ||
                      quickBusy ||
                      selected.read_only ||
                      inbound === null
                    }
                    onClick={() => void openAction("create")}
                  >
                    <Plus size={15} /> Новый клиент
                  </button>
                </div>
                {selected.read_only && (
                  <div className="sui-readonly-note">
                    <ShieldCheck size={15} />
                    <p>
                      Сейчас доступен просмотр и экспорт. Создание можно
                      разрешить в настройках панели.
                    </p>
                  </div>
                )}
                {!listed ? (
                  <div className="sui-empty-clients">
                    <KeyRound size={29} />
                    <h3>Подключитесь к вашей панели</h3>
                    <p>
                      API-токен понадобится только для текущего запроса.
                      <br />
                      Просмотр списка не изменяет настройки VPN.
                    </p>
                    <div>
                      <button
                        className="button"
                        disabled={!native || quickBusy}
                        onClick={() => void openAction("inspect")}
                      >
                        Проверить доступ
                      </button>
                      <button
                        className="button primary"
                        disabled={!native || quickBusy}
                        onClick={() => void openAction("list")}
                      >
                        Загрузить клиентов <ArrowRight size={16} />
                      </button>
                    </div>
                  </div>
                ) : visible.length ? (
                  <div className="sui-client-list">
                    <div className="sui-client-list-label">
                      <span>Клиент</span>
                      <span>Передано</span>
                      <span>Профиль</span>
                    </div>
                    {visible.map((client) => (
                      <div className="sui-client-row" key={client.id}>
                        <span className="sui-device-icon">
                          {/phone|телефон/i.test(client.name) ? (
                            <Smartphone size={20} />
                          ) : (
                            <UserRound size={20} />
                          )}
                        </span>
                        <div className="sui-client-name">
                          <strong>
                            {client.name || `Клиент ${client.id}`}
                          </strong>
                          <small>
                            <i className={client.enable ? "enabled" : ""} />
                            {client.enable
                              ? "Доступ включён"
                              : "Доступ отключён"}
                          </small>
                        </div>
                        <span className="sui-client-traffic">
                          {bytes(client.up + client.down)}
                        </span>
                        <button
                          className="button sui-qr-button"
                          disabled={!native || quickBusy || !client.enable}
                          onClick={() => void openAction("export", client)}
                        >
                          <QrCode size={16} />
                          <span>QR-код</span>
                        </button>
                      </div>
                    ))}
                  </div>
                ) : (
                  <div className="sui-empty-clients">
                    <Users size={29} />
                    <h3>Для этого подключения клиентов нет</h3>
                    <p>
                      Выберите другое подключение или создайте нового клиента.
                    </p>
                  </div>
                )}
                <div className="sui-panel-footer">
                  <Workflow size={15} />
                  <span>
                    Создание клиента через S-UI может переподключить действующие
                    VPN-сеансы.
                  </span>
                </div>
              </section>
            ) : (
              <section className="card sui-empty-clients">
                <Layers3 size={32} />
                <h3>Выберите панель слева</h3>
                <p>Управляйте клиентами установленной S-UI.</p>
              </section>
            )}
          </div>
        </>
      )}
      {config && (
        <SuiSettings
          active={active}
          initial={config.value}
          panelId={config.id}
          credentialSaved={Boolean(
            panels.find((panel) => panel.id === config.id)?.credential_saved,
          )}
          onForgot={() => config.id && credentialChanged(config.id, false)}
          onClose={() => setConfig(null)}
          onSaved={(panel) => {
            setPanels((previous) => [
              ...previous.filter((item) => item.id !== panel.id),
              panel,
            ]);
            setSelected(panel);
            setConfig(null);
            setInbounds([]);
            setInbound(null);
            setClients([]);
            setListed(false);
            onNotice("Настройки панели сохранены на этом компьютере");
          }}
        />
      )}
      {access && selected && (
        <SuiAccess
          active={active}
          panel={selected}
          action={access.action}
          inbound={inbound}
          client={access.client}
          forceToken={access.forceToken}
          onCredentialSaved={() => credentialChanged(selected.id, true)}
          onClose={() => setAccess(null)}
          onResult={receive}
          onBusyChange={onBusyChange}
        />
      )}
      {share && (
        <ShareDialog
          active={active}
          client={{ ...share, id: String(share.id) }}
          onClose={() => setShare(null)}
          onImport={onImport}
          onNotice={onNotice}
        />
      )}
    </div>
  );
}

function SuiSettings({
  active,
  initial,
  panelId,
  credentialSaved,
  onForgot,
  onClose,
  onSaved,
}: {
  active: boolean;
  initial: SuiConfig;
  panelId?: string;
  credentialSaved: boolean;
  onForgot: () => void;
  onClose: () => void;
  onSaved: (panel: SuiPanelInfo) => void;
}) {
  const [value, setValue] = useState(initial);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  function update(key: keyof SuiConfig, field: string | number | boolean) {
    setValue((previous) => ({ ...previous, [key]: field }));
  }
  async function forgetCredential() {
    if (!panelId) return;
    setError("");
    setBusy(true);
    try {
      await runOperation("/v1/sui/credentials/forget", { panel_id: panelId });
      onForgot();
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  }
  async function save(event: FormEvent) {
    event.preventDefault();
    const invalid = suiConfigError(value, Boolean(panelId));
    if (invalid) {
      setError(invalid);
      return;
    }
    setBusy(true);
    setError("");
    try {
      const result = await runOperation<{ panel: SuiPanelInfo }>(
        "/v1/sui/panels/save",
        {
          ...(panelId ? { panel_id: panelId } : {}),
          name: value.name.trim(),
          ...(value.base_url.trim() ? { base_url: value.base_url.trim() } : {}),
          server_domain: value.server_domain.trim(),
          server_port: value.server_port,
          read_only: value.read_only,
        },
      );
      setValue(blankConfig());
      onSaved(result.panel);
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  }
  return (
    <Dialog
      active={active}
      title={panelId ? "Настройки S-UI" : "Добавить панель S-UI"}
      onClose={() => {
        setValue(blankConfig());
        onClose();
      }}
      busy={busy}
      className="sui-settings-dialog"
    >
      <form onSubmit={save}>
        <p className="sh-dialog-intro">
          Сохраним адрес панели и публичный адрес VPN.
          <br />
          {credentialSaved
            ? "Для запросов используется токен из хранилища Windows."
            : "API-токен будет запрошен отдельно перед подключением."}
        </p>
        {panelId && (
          <div className="sui-vault-settings">
            <div>
              <ShieldCheck size={18} />
              <span>
                {credentialSaved
                  ? "Токен сохранён в Windows"
                  : "Токен не сохранён"}
              </span>
            </div>
            {credentialSaved && (
              <button
                type="button"
                className="button"
                disabled={!native || busy}
                onClick={() => void forgetCredential()}
              >
                Забыть токен
              </button>
            )}
          </div>
        )}
        {error && (
          <div className="sh-error" role="alert">
            <AlertCircle size={17} />
            <p>{error}</p>
          </div>
        )}
        <div className="sh-field">
          <label htmlFor="sui-name">Название панели</label>
          <input
            id="sui-name"
            value={value.name}
            onChange={(e) => update("name", e.target.value)}
            placeholder="Например, мой сервер в Финляндии"
            required
            maxLength={80}
            disabled={busy}
          />
        </div>
        <div className="sh-field">
          <label htmlFor="sui-url">Полный адрес панели</label>
          <input
            id="sui-url"
            type="url"
            autoComplete="off"
            spellCheck={false}
            value={value.base_url}
            onChange={(e) => update("base_url", e.target.value)}
            placeholder={
              panelId
                ? "Оставьте пустым, чтобы сохранить адрес"
                : "https://panel.example.com/секретный-путь/"
            }
            required={!panelId}
            disabled={busy}
          />
          <small>
            {panelId
              ? "Введите новый адрес, только если панель переехала."
              : "Адрес вместе с портом и путём, по которому вы открываете S-UI."}
          </small>
        </div>
        <div className="sui-address-grid">
          <div className="sh-field">
            <label htmlFor="sui-domain">Публичный домен VPN</label>
            <input
              id="sui-domain"
              value={value.server_domain}
              onChange={(e) => update("server_domain", e.target.value)}
              placeholder="vpn.example.com"
              autoComplete="off"
              spellCheck={false}
              required
              disabled={busy}
            />
          </div>
          <div className="sh-field">
            <label htmlFor="sui-port">Порт VPN</label>
            <input
              id="sui-port"
              type="number"
              min={1}
              max={65535}
              value={value.server_port}
              onChange={(e) => update("server_port", Number(e.target.value))}
              required
              disabled={busy}
            />
          </div>
        </div>
        <p className="sui-public-address-note">
          <Globe2 size={14} /> При HTTPS через Caddy обычно нужен публичный порт
          443. Внутренний порт S-UI в профиль не попадёт.
        </p>
        <label className="sh-check-label sui-write-check">
          <input
            type="checkbox"
            checked={!value.read_only}
            onChange={(e) => update("read_only", !e.target.checked)}
            disabled={busy}
          />
          <span>
            Разрешить создание клиентов из Corepass
            <small>По умолчанию — только просмотр и экспорт.</small>
          </span>
        </label>
        {!value.read_only && (
          <div className="sui-create-warning">
            <AlertCircle size={17} />
            <p>
              S-UI может перезапустить VPN при добавлении клиента. Активные
              соединения могут прерваться.
            </p>
          </div>
        )}
        <div className="sh-form-actions">
          <button
            type="button"
            className="button"
            disabled={busy}
            onClick={() => {
              setValue(blankConfig());
              onClose();
            }}
          >
            Отмена
          </button>
          <button
            type="submit"
            className="button primary"
            disabled={!native || busy}
          >
            {busy && <LoaderCircle size={16} className="sh-spinner" />}
            {busy ? "Сохраняем…" : "Сохранить панель"}
          </button>
        </div>
      </form>
    </Dialog>
  );
}

function SuiAccess({
  active,
  panel,
  action,
  inbound,
  client,
  forceToken,
  onCredentialSaved,
  onClose,
  onResult,
  onBusyChange,
}: {
  active: boolean;
  panel: SuiPanelInfo;
  action: "inspect" | "list" | "create" | "export";
  inbound: number | null;
  client?: SuiClient;
  forceToken?: boolean;
  onCredentialSaved: () => void;
  onClose: () => void;
  onResult: (result: SuiResult) => void;
  onBusyChange: (busy: boolean) => void;
}) {
  const [token, setToken] = useState("");
  const [stored, setStored] = useState(
    !forceToken && Boolean(panel.credential_saved),
  );
  const [remember, setRemember] = useState(false);
  const [showToken, setShowToken] = useState(false);
  const [name, setName] = useState("");
  const [confirmed, setConfirmed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    onBusyChange(busy);
    return () => onBusyChange(false);
  }, [busy, onBusyChange]);
  const title = {
    inspect: "Проверить панель",
    list: "Загрузить клиентов",
    create: "Новый клиент S-UI",
    export: "Получить профиль",
  }[action];
  const close = () => {
    setToken("");
    setName("");
    setConfirmed(false);
    onClose();
  };
  const allowed =
    action === "create"
      ? mayCreateClient(
          panel,
          stored ? "stored-credential" : token,
          name,
          inbound,
          confirmed,
        )
      : (stored || Boolean(token.trim())) &&
        (action !== "export" || inbound !== null);
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!allowed) return;
    setBusy(true);
    setError("");
    try {
      const result = await performSuiAction(
        {
          panelId: panel.id,
          action,
          stored,
          remember,
          token,
          inbound,
          clientId: client?.id,
          clientName: name,
          confirmed,
        },
        {
          onCredentialSaved: () => {
            setStored(true);
            setToken("");
            onCredentialSaved();
          },
        },
      );
      setToken("");
      setName("");
      onResult(result);
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
      onClose={close}
      busy={busy}
      className="sui-access-dialog"
    >
      <form onSubmit={submit}>
        <p className="sh-dialog-intro">
          {action === "create" ? (
            "Создадим отдельный доступ для вашего устройства."
          ) : action === "export" ? (
            <>
              Профиль клиента <strong>{client?.name}</strong> будет получен из
              вашей панели.
            </>
          ) : (
            <>
              Подключение к <strong>{panel.name}</strong>
              <br />
              {panel.panel_host}
            </>
          )}
        </p>
        {error && (
          <div className="sh-error" role="alert">
            <AlertCircle size={17} />
            <p>{error}</p>
          </div>
        )}
        {action === "create" && (
          <div className="sh-field">
            <label htmlFor="sui-client-name">Название устройства</label>
            <input
              id="sui-client-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="Например, iPhone"
              maxLength={64}
              required
              disabled={busy}
            />
          </div>
        )}
        {stored ? (
          <div className="sui-stored-credential">
            <div>
              <ShieldCheck size={20} />
              <span>
                <strong>Токен сохранён в Windows</strong>
                <small>Будет использован для запроса к этой панели.</small>
              </span>
            </div>
            <button
              type="button"
              className="sui-text-button"
              disabled={busy}
              onClick={() => {
                setStored(false);
                setRemember(false);
              }}
            >
              Ввести другой токен
            </button>
          </div>
        ) : (
          <>
            <div className="sh-field">
              <label htmlFor="sui-api-token">API-токен S-UI</label>
              <div className="sh-password-field">
                <input
                  id="sui-api-token"
                  maxLength={2048}
                  type={showToken ? "text" : "password"}
                  value={token}
                  onChange={(e) => setToken(e.target.value)}
                  autoComplete="off"
                  spellCheck={false}
                  placeholder="Вставьте API-токен вашей панели"
                  required
                  disabled={busy}
                />
                <button
                  type="button"
                  className="icon-button"
                  aria-label={
                    showToken ? "Скрыть API-токен" : "Показать API-токен"
                  }
                  onClick={() => setShowToken(!showToken)}
                >
                  {showToken ? <EyeOff size={17} /> : <Eye size={17} />}
                </button>
              </div>
              <small>
                Без сохранения токен останется только в памяти этого окна.
              </small>
            </div>
            <label className="sh-check-label sui-remember-token">
              <input
                type="checkbox"
                checked={remember}
                onChange={(event) => setRemember(event.target.checked)}
                disabled={busy}
              />
              <span>
                Запомнить в хранилище Windows
                <small>Токен можно удалить в настройках этой панели.</small>
              </span>
            </label>
          </>
        )}
        {action === "create" && (
          <>
            <div className="sui-create-warning">
              <AlertCircle size={19} />
              <div>
                <strong>Возможно переподключение VPN</strong>
                <p>
                  S-UI может перезапустить подключения сервера при создании
                  клиента. Сделайте это, когда короткий перерыв допустим.
                </p>
              </div>
            </div>
            <label className="sh-check-label">
              <input
                type="checkbox"
                checked={confirmed}
                onChange={(e) => setConfirmed(e.target.checked)}
                disabled={busy}
              />
              <span>
                Создать клиента и разрешить возможное переподключение
                действующих сеансов.
              </span>
            </label>
          </>
        )}
        <div className="sh-secret-note">
          <LockKeyhole size={14} /> Прямой запрос к вашей панели. Облачная
          панель Corepass не используется.
        </div>
        <div className="sh-form-actions">
          <button
            type="button"
            className="button"
            onClick={close}
            disabled={busy}
          >
            Отмена
          </button>
          <button
            type="submit"
            className="button primary"
            disabled={!native || busy || !allowed}
          >
            {busy && <LoaderCircle size={16} className="sh-spinner" />}
            {busy
              ? "Выполняем…"
              : action === "create"
                ? "Создать клиента"
                : action === "export"
                  ? "Получить QR-код"
                  : action === "inspect"
                    ? "Проверить доступ"
                    : "Загрузить клиентов"}
          </button>
        </div>
      </form>
    </Dialog>
  );
}
