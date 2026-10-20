import { describe, expect, it } from "vitest";
import {
  connectionError,
  runOperation,
  type ConnectionForm,
  type Transport,
} from "./server-api";

const valid: ConnectionForm = {
  host: "203.0.113.10",
  ssh_port: 22,
  username: "root",
  password: "example-password",
  domain: "vpn.example.com",
};
describe("server setup input", () => {
  it("requires a real port, root access and a DNS domain before any request", () => {
    expect(connectionError(valid)).toBe("");
    expect(connectionError({ ...valid, ssh_port: 0 })).toContain("Порт SSH");
    expect(connectionError({ ...valid, ssh_port: 65536 })).toContain(
      "Порт SSH",
    );
    expect(connectionError({ ...valid, username: "user" })).toContain("root");
    expect(connectionError({ ...valid, password: "" })).toContain("пароль");
    expect(
      connectionError({ ...valid, domain: "https://vpn.example.com/path" }),
    ).toContain("домен");
    expect(connectionError({ ...valid, host: "host; command" })).toContain(
      "IPv4",
    );
  });
});

describe("operation result handoff", () => {
  it("returns the completed result rather than the accepted operation envelope", async () => {
    const paths: string[] = [];
    const values: unknown[] = [
      { operation_id: "srv_123" },
      { state: "running" },
      {
        state: "succeeded",
        result: { path: "report.json", clients: ["phone"] },
      },
    ];
    const transport: Transport = async <T>(_method: string, path: string) => {
      paths.push(path);
      return values.shift() as T;
    };
    const result = await runOperation(
      "/v1/servers/install",
      {},
      { transport, delay: async () => {} },
    );
    expect(result).toEqual({ path: "report.json", clients: ["phone"] });
    expect(paths).toEqual([
      "/v1/servers/install",
      "/v1/operations/srv_123",
      "/v1/operations/srv_123",
    ]);
  });
  it("surfaces a failed operation and never emits success data", async () => {
    const values: unknown[] = [
      { operation_id: "srv_failed" },
      { state: "failed", error: { message: "Ключ сервера изменился" } },
    ];
    const transport: Transport = async <T>() => values.shift() as T;
    await expect(
      runOperation(
        "/v1/servers/inspect",
        {},
        { transport, delay: async () => {} },
      ),
    ).rejects.toThrow("Ключ сервера изменился");
  });
  it("preserves immediate command results and reports a missing final result", async () => {
    const immediate: Transport = async <T>() => ({ path: "report.json" }) as T;
    expect(
      await runOperation("/v1/support/export", {}, { transport: immediate }),
    ).toEqual({ path: "report.json" });
    const values: unknown[] = [
      { operation_id: "srv_empty" },
      { state: "succeeded" },
    ];
    const empty: Transport = async <T>() => values.shift() as T;
    await expect(
      runOperation(
        "/v1/servers/install",
        {},
        { transport: empty, delay: async () => {} },
      ),
    ).rejects.toThrow("не вернула результат");
  });
  it("does not invite retry while the server may still be working", async () => {
    const transport: Transport = async <T>() =>
      ({ operation_id: "srv_pending" }) as T;
    await expect(
      runOperation("/v1/servers/install", {}, { transport, timeoutMs: 0 }),
    ).rejects.toThrow("Не запускайте её повторно");
  });
  it("stops polling when the backend has expired its retained result", async () => {
    const values: unknown[] = [
      { operation_id: "srv_expired" },
      { state: "expired" },
    ];
    const transport: Transport = async <T>() => values.shift() as T;
    await expect(
      runOperation(
        "/v1/servers/install",
        {},
        { transport, delay: async () => {} },
      ),
    ).rejects.toThrow("Проверьте состояние сервера");
  });
});
