import { describe, expect, it } from "vitest";
import { ageParts, exitCodeText, hostPort, localDateTime, middleEllipsis } from "./format";

describe("format", () => {
  const now = Date.parse("2026-10-17T15:00:00Z");

  it("ages", () => {
    expect(ageParts("2026-10-17T14:59:30Z", now)).toEqual({ value: 30, unit: "s" });
    expect(ageParts("2026-10-17T14:24:00Z", now)).toEqual({ value: 36, unit: "min" });
    expect(ageParts("2026-10-17T12:00:00Z", now)).toEqual({ value: 3, unit: "h" });
    expect(ageParts("2026-10-15T15:00:00Z", now)).toEqual({ value: 2, unit: "d" });
    expect(ageParts("2026-10-17T16:00:00Z", now)).toEqual({ value: 0, unit: "s" });
    expect(ageParts(null, now)).toBeNull();
    expect(ageParts("garbage", now)).toBeNull();
  });

  it("dates", () => {
    expect(localDateTime("2026-10-17T14:22:03Z", "en-US")).toContain("2026");
    expect(localDateTime(null, "en-US")).toBe("");
  });

  it("host and port", () => {
    expect(hostPort("127.0.0.1", 3000)).toBe("127.0.0.1:3000");
    expect(hostPort("::1", 3000)).toBe("[::1]:3000");
  });

  it("middle ellipsis keeps both ends", () => {
    expect(middleEllipsis("short", 10)).toBe("short");
    const s = middleEllipsis("C:\\Users\\dev\\projects\\very-long-name\\apps\\web", 24);
    expect(s.length).toBe(24);
    expect(s.startsWith("C:\\Users")).toBe(true);
    expect(s.endsWith("apps\\web")).toBe(true);
  });

  it("exit codes", () => {
    expect(exitCodeText(0)).toBe("0");
    expect(exitCodeText(3)).toBe("3");
    expect(exitCodeText(-1073741510)).toBe("0xC000013A");
  });
});
