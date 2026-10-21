import { api } from "./api";

export type ManagedServer = {
  id: string;
  name: string;
  host: string;
  ssh_port: number;
  username: string;
  domain: string;
  fingerprint: string;
  created_at: string;
};
export type ServerClient = {
  id: string;
  name: string;
  created_at?: string;
  revoked?: boolean;
};
export type ClientExport = { id: string; name: string; uri: string };
export type ConnectionForm = {
  host: string;
  ssh_port: number;
  username: string;
  password: string;
  domain: string;
};
export type Inspection = {
  plan_id: string;
  expires_in_seconds: number;
  ready: boolean;
  installed: boolean;
  checks: { id: string; label: string; ok: boolean; message: string }[];
  blockers: string[];
  domain: string;
  host: string;
  architecture: string;
  os: string;
};
type Operation<T> = {
  state: string;
  error?: string | { message?: string };
  result?: T;
};
export type Transport = <T>(
  method: string,
  path: string,
  body?: object,
) => Promise<T>;

// Long operations expose their final result only through the operation endpoint.
// In particular, exports and provisioning must never treat the accepted job as success.
export async function runOperation<T>(
  path: string,
  body: object = {},
  options: {
    transport?: Transport;
    delay?: (ms: number) => Promise<void>;
    timeoutMs?: number;
    onState?: (state: string) => void;
  } = {},
): Promise<T> {
  const request = options.transport ?? api;
  const sleep =
    options.delay ??
    ((ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms)));
  const result = await request<T & { operation_id?: string }>(
    "POST",
    path,
    body,
  );
  if (!result.operation_id) return result;
  const deadline = Date.now() + (options.timeoutMs ?? 180_000);
  while (Date.now() < deadline) {
    await sleep(400);
    const operation = await request<Operation<T>>(
      "GET",
      `/v1/operations/${result.operation_id}`,
    );
    options.onState?.(operation.state);
    if (operation.state === "expired")
      throw new Error(
        "Результат операции уже истёк. Проверьте состояние сервера перед повторным действием.",
      );
    if (["failed", "cancelled"].includes(operation.state)) {
      const detail =
        typeof operation.error === "string"
          ? operation.error
          : operation.error?.message;
      throw new Error(detail || "Операция не выполнена. Проверьте журнал.");
    }
    if (operation.state === "succeeded") {
      if (operation.result === undefined)
        throw new Error("Операция не вернула результат.");
      return operation.result;
    }
  }
  throw new Error(
    "Сервер ещё выполняет операцию. Не запускайте её повторно: дождитесь результата в журнале.",
  );
}

export function connectionError(form: ConnectionForm): string {
  if (!/^[a-zA-Z0-9][a-zA-Z0-9.-]{0,252}$/.test(form.host.trim()))
    return "Укажите IPv4 или имя SSH-сервера без https:// и пути.";
  if (
    !Number.isInteger(form.ssh_port) ||
    form.ssh_port < 1 ||
    form.ssh_port > 65535
  )
    return "Порт SSH должен быть от 1 до 65535.";
  if (form.username !== "root")
    return "В первой версии установка доступна с пользователем root.";
  if (!form.password || form.password.length > 1024)
    return "Введите пароль SSH-сервера.";
  if (
    !/^(?=.{1,253}$)(?:[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?\.)+[a-zA-Z]{2,63}$/.test(
      form.domain.trim(),
    )
  )
    return "Укажите домен VPN, например vpn.example.com. Его DNS должен вести на сервер.";
  return "";
}
