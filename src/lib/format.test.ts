import { describe, expect, it } from "vitest";
import { formatSize } from "./format";

describe("formatSize", () => {
  it("uses the language's decimal separator", () => {
    const bytes = 12.3 * 1024 ** 3;
    expect(formatSize(bytes, "es")).toBe("12,3 GB");
    expect(formatSize(bytes, "en")).toBe("12.3 GB");
  });

  it("drops decimals for bytes and large values", () => {
    expect(formatSize(512, "en")).toBe("512 B");
    expect(formatSize(150 * 1024 ** 2, "en")).toBe("150 MB");
  });
});
