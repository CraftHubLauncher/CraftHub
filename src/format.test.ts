import { describe, expect, it } from "vitest";
import {
  formatBytes,
  formatRelative,
  isActivePhase,
  matchesQuery,
  progressFraction,
  verificationLabel,
} from "./format";
import { app } from "./test/fixtures";

describe("format helpers", () => {
  it("formats bytes", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(65251740)).toBe("62 MB");
    expect(formatBytes(-1)).toBe("—");
  });

  it("formats relative times", () => {
    const now = 1_000_000 * 1000;
    expect(formatRelative(null, now)).toBe("never");
    expect(formatRelative(1_000_000 - 10, now)).toBe("just now");
    expect(formatRelative(1_000_000 - 7200, now)).toBe("2 h ago");
  });

  it("only reports measurable progress for byte-counted phases", () => {
    const base = { opId: "o", appId: "a", kind: "install" as const, message: null };
    expect(progressFraction({ ...base, phase: "downloading", done: 1, total: 4 })).toBe(0.25);
    expect(progressFraction({ ...base, phase: "verifying", done: 0, total: 1 })).toBeNull();
    expect(progressFraction({ ...base, phase: "downloading", done: 0, total: 0 })).toBeNull();
    expect(isActivePhase("extracting")).toBe(true);
    expect(isActivePhase("failed")).toBe(false);
  });

  it("never describes digests as signatures", () => {
    for (const v of ["github-digest", "github-digest+sha256sums"]) {
      expect(verificationLabel(v).toLowerCase()).not.toContain("sign");
    }
  });

  it("searches by name and description", () => {
    const a = app("photocraft", "PhotoCraft");
    expect(matchesQuery(a, "photo")).toBe(true);
    expect(matchesQuery(a, "description")).toBe(true);
    expect(matchesQuery(a, "vector")).toBe(false);
  });
});
