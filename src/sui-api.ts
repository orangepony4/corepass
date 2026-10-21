export type SuiPanelInfo = {
  id: string;
  name: string;
  panel_host: string;
  server_domain: string;
  server_port: number;
  read_only: boolean;
  credential_saved?: boolean;
  created_at: string;
};
export type SuiInbound = {
  id: number;
  tag: string;
  type: "naive";
  listen_port: number;
  client_count: number;
};
export type SuiClient = {
  id: number;
  name: string;
  enable: boolean;
  inbounds: number[];
  expiry: number;
  volume: number;
  up: number;
  down: number;
};
export type SuiClientResult = { id: number; name: string; uri: string };
export type SuiResult = {
  panel?: SuiPanelInfo;
  clients?: SuiClient[];
  inbounds?: SuiInbound[];
  client?: SuiClientResult;
  warning?: string;
};
export type SuiConfig = {
  name: string;
  base_url: string;
  server_domain: string;
  server_port: number;
  read_only: boolean;
};

// Credential storage and a remote mutation are separate explicit actions.
// A failed save or an ambiguous remote response must never cause a retry.
export async function performSuiAction(
  input: {
    panelId: string;
    action: SuiAction;
    token?: string;
    stored?: boolean;
    remember?: boolean;
    inbound?: number | null;
    clientId?: number;
    clientName?: string;
    confirmed?: boolean;
  },
  options: {
    request?: (path: string, body: Record<string, unknown>) => Promise<unknown>;
    onCredentialSaved?: () => void;
  } = {},
): Promise<SuiResult> {
  const request = options.request ?? ((path, body) => runOperation(path, body));
  const token = input.token?.trim() ?? "";
  if (!input.stored && !token) throw new Error("Введите API-токен панели.");
  if (input.action === "create" && !input.confirmed)
    throw new Error(
      "Подтвердите возможное переподключение VPN перед созданием клиента.",
    );
  const useVault = Boolean(input.stored || input.remember);
  if (!input.stored && input.remember) {
    await request("/v1/sui/credentials/save", {
      panel_id: input.panelId,
      token,
    });
    options.onCredentialSaved?.();
  }
  const route =
    input.action === "inspect"
      ? "/v1/sui/inspect"
      : `/v1/sui/clients/${input.action}`;
  return (await request(route, {
    panel_id: input.panelId,
    ...(useVault ? {} : { token }),
    ...(input.action === "create"
      ? {
          inbound_id: input.inbound,
          client_name: input.clientName?.trim(),
          confirm_create: true,
        }
      : {}),
    ...(input.action === "export"
      ? { client_id: input.clientId, inbound_id: input.inbound }
      : {}),
  })) as SuiResult;
}

export function suiConfigError(value: SuiConfig, editing = false): string {
  if (!value.name.trim() || value.name.trim().length > 80)
    return "Название панели должно содержать от 1 до 80 символов.";
  if (!editing || value.base_url.trim()) {
    try {
      const url = new URL(value.base_url.trim());
      if (
        url.protocol !== "https:" ||
        !url.hostname ||
        url.username ||
        url.password ||
        url.hash ||
        url.search
      )
        throw new Error();
    } catch {
      return "Введите полный HTTPS-адрес панели без логина, пароля, параметров и #.";
    }
  }
  if (
    !/^(?=.{1,253}$)(?:[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?\.)+[a-zA-Z]{2,63}$/.test(
      value.server_domain.trim(),
    )
  )
    return "Укажите публичный домен VPN, например vpn.example.com.";
  if (
    !Number.isInteger(value.server_port) ||
    value.server_port < 1 ||
    value.server_port > 65535
  )
    return "Публичный порт VPN должен быть от 1 до 65535.";
  return "";
}

export function clientsForInbound(
  clients: SuiClient[],
  inbound: number | null,
): SuiClient[] {
  return inbound === null
    ? []
    : clients.filter((client) => client.inbounds.includes(inbound));
}

export function mayCreateClient(
  panel: SuiPanelInfo,
  token: string,
  name: string,
  inbound: number | null,
  confirmed: boolean,
): boolean {
  return (
    !panel.read_only &&
    Boolean(token.trim()) &&
    Boolean(name.trim()) &&
    name.trim().length <= 64 &&
    inbound !== null &&
    Number.isInteger(inbound) &&
    confirmed
  );
}
import { runOperation } from "./server-api";

export type SuiAction = "inspect" | "list" | "create" | "export";
