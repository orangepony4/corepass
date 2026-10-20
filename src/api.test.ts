import { describe, it, expect } from "vitest";
import { bytes, duration } from "./api";
describe("traffic display", () => {
  it("does not render broken or negative telemetry", () => {
    expect(bytes(NaN)).toBe("—");
    expect(bytes(-1)).toBe("—");
    expect(bytes(Infinity)).toBe("—");
  });
  it("converts byte unit boundaries consistently", () => {
    expect(bytes(0)).toBe("0 Б");
    expect(bytes(1024)).toBe("1.0 КБ");
    expect(bytes(1048576)).toBe("1.0 МБ");
    expect(bytes(1073741824)).toBe("1.00 ГБ");
  });
  it("keeps uptime readable across an hour boundary", () => {
    expect(duration(3599)).toBe("00:59:59");
    expect(duration(3600)).toBe("01:00:00");
  });
});
