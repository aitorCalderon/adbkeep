import { describe, expect, it } from "vitest";
import en from "./en.json";
import es from "./es.json";

function keys(obj: object, prefix = ""): string[] {
  return Object.entries(obj).flatMap(([k, v]) =>
    v && typeof v === "object" ? keys(v, `${prefix}${k}.`) : [`${prefix}${k}`],
  );
}

describe("translations", () => {
  it("es and en have the same keys", () => {
    expect(keys(es).sort()).toEqual(keys(en).sort());
  });
});
