import { describe, expect, it } from "vitest";
import {
  clientsForInbound,
  mayCreateClient,
  suiConfigError,
  performSuiAction,
  type SuiClient,
  type SuiConfig,
  type SuiPanelInfo,
} from "./sui-api";

const config: SuiConfig = {
  name: "My panel",
  base_url: "https://panel.example.com/private-path/",
  server_domain: "vpn.example.com",
  server_port: 443,
  read_only: true,
};
const panel: SuiPanelInfo = {
  id: "example",
  name: "My panel",
  panel_host: "panel.example.com",
  server_domain: "vpn.example.com",
  server_port: 443,
  read_only: true,
  created_at: "2026-10-21T12:00:00Z",
};

describe("S-UI configuration", () => {
  it("requires HTTPS and keeps token, credentials and query parameters out of panel URLs", () => {
    expect(suiConfigError(config)).toBe("");
    for (const base_url of [
      "http://panel.example.com/",
      "file:///config",
      "https://user:pass@panel.example.com/",
      "https://panel.example.com/?token=secret",
      "https://panel.example.com/#token",
    ]) {
      expect(suiConfigError({ ...config, base_url })).toContain("HTTPS");
    }
  });
  it("requires explicit public VPN endpoint and permits editing without exposing a saved secret path", () => {
    expect(suiConfigError({ ...config, base_url: "" })).toContain("HTTPS");
    expect(suiConfigError({ ...config, base_url: "" }, true)).toBe("");
    expect(
      suiConfigError({ ...config, server_domain: "https://vpn.example.com" }),
    ).toContain("домен");
    expect(suiConfigError({ ...config, server_port: 0 })).toContain("порт");
    expect(suiConfigError({ ...config, server_port: 65536 })).toContain("порт");
  });
});

describe("S-UI credential handoff", () => {
  it("does not save a token without explicit opt-in", async () => {
    const calls: { path: string; body: Record<string, unknown> }[] = [];
    const request = async (path: string, body: Record<string, unknown>) => {
      calls.push({ path, body });
      return { clients: [] };
    };
    await performSuiAction(
      { panelId: "example", action: "list", token: "temporary-token" },
      { request },
    );
    expect(calls).toEqual([
      {
        path: "/v1/sui/clients/list",
        body: { panel_id: "example", token: "temporary-token" },
      },
    ]);
  });
  it("saves opted-in credentials first and omits the token from the subsequent remote operation", async () => {
    const calls: { path: string; body: Record<string, unknown> }[] = [];
    const request = async (path: string, body: Record<string, unknown>) => {
      calls.push({ path, body });
      return {};
    };
    await performSuiAction(
      {
        panelId: "example",
        action: "export",
        token: "save-me",
        remember: true,
        inbound: 1,
        clientId: 2,
      },
      { request },
    );
    expect(calls.map((call) => call.path)).toEqual([
      "/v1/sui/credentials/save",
      "/v1/sui/clients/export",
    ]);
    expect(calls[0].body.token).toBe("save-me");
    expect(calls[1].body).toEqual({
      panel_id: "example",
      inbound_id: 1,
      client_id: 2,
    });
  });
  it("does not contact the panel after credential storage fails", async () => {
    const paths: string[] = [];
    const request = async (path: string) => {
      paths.push(path);
      throw new Error("Windows vault unavailable");
    };
    await expect(
      performSuiAction(
        {
          panelId: "example",
          action: "list",
          token: "temporary",
          remember: true,
        },
        { request },
      ),
    ).rejects.toThrow("Windows vault unavailable");
    expect(paths).toEqual(["/v1/sui/credentials/save"]);
  });
  it("uses stored credentials without a secret in the request and never retries an ambiguous create", async () => {
    const calls: Record<string, unknown>[] = [];
    const request = async (_path: string, body: Record<string, unknown>) => {
      calls.push(body);
      throw new Error("Connection closed");
    };
    await expect(
      performSuiAction(
        {
          panelId: "example",
          action: "create",
          stored: true,
          inbound: 1,
          clientName: "Phone",
          confirmed: true,
        },
        { request },
      ),
    ).rejects.toThrow("Connection closed");
    expect(calls).toEqual([
      {
        panel_id: "example",
        inbound_id: 1,
        client_name: "Phone",
        confirm_create: true,
      },
    ]);
  });
  it("blocks an unconfirmed create before any credential write or remote request", async () => {
    let contacted = false;
    await expect(
      performSuiAction(
        {
          panelId: "example",
          action: "create",
          token: "token",
          remember: true,
          inbound: 1,
          clientName: "Phone",
          confirmed: false,
        },
        {
          request: async () => {
            contacted = true;
            return {};
          },
        },
      ),
    ).rejects.toThrow("Подтвердите");
    expect(contacted).toBe(false);
  });
});
describe("S-UI client boundaries", () => {
  it("shows only clients of the selected inbound", () => {
    const base = { enable: true, expiry: 0, volume: 0, up: 0, down: 0 };
    const clients: SuiClient[] = [
      { ...base, id: 1, name: "Desktop", inbounds: [1] },
      { ...base, id: 2, name: "Phone", inbounds: [2] },
      { ...base, id: 3, name: "Shared", inbounds: [1, 2] },
    ];
    expect(clientsForInbound(clients, 1).map((client) => client.id)).toEqual([
      1, 3,
    ]);
    expect(clientsForInbound(clients, null)).toEqual([]);
  });
  it("creation needs saved write access, a token, an inbound, a name and explicit disruption confirmation", () => {
    expect(mayCreateClient(panel, "example-token", "Phone", 1, true)).toBe(
      false,
    );
    const writable = { ...panel, read_only: false };
    expect(mayCreateClient(writable, "example-token", "Phone", 1, true)).toBe(
      true,
    );
    expect(mayCreateClient(writable, "example-token", "Phone", 1, false)).toBe(
      false,
    );
    expect(mayCreateClient(writable, "", "Phone", 1, true)).toBe(false);
    expect(mayCreateClient(writable, "example-token", "", 1, true)).toBe(false);
    expect(
      mayCreateClient(writable, "example-token", "Phone", null, true),
    ).toBe(false);
  });
});
