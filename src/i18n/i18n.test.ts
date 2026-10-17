import { describe, expect, it } from "vitest";
import { en } from "./en";
import { LANGUAGES, resolveLang } from "./languages";
import { tr } from "./tr";

type Tree = { [k: string]: string | Tree };

function flatten(tree: Tree, prefix = ""): Map<string, string> {
  const out = new Map<string, string>();
  for (const [k, v] of Object.entries(tree)) {
    const key = prefix ? `${prefix}.${k}` : k;
    if (typeof v === "string") out.set(key, v);
    else for (const [kk, vv] of flatten(v, key)) out.set(kk, vv);
  }
  return out;
}

const vars = (s: string) => [...new Set([...s.matchAll(/\{\{(\w+)\}\}/g)].map((m) => m[1]))].sort();
const emoji = /\p{Extended_Pictographic}/u;

const enMap = flatten(en as unknown as Tree);
const trMap = flatten(tr as unknown as Tree);

describe("Turkish", () => {
  it("has exactly the English keys", () => {
    expect([...trMap.keys()].sort()).toEqual([...enMap.keys()].sort());
  });

  it("uses the same interpolation variables", () => {
    for (const [key, value] of enMap) {
      expect({ key, vars: vars(trMap.get(key) ?? "") }).toEqual({ key, vars: vars(value) });
    }
  });
});

describe.each([
  ["en", enMap],
  ["tr", trMap],
])("%s", (_code, map) => {
  it("has no empty strings and no emoji", () => {
    for (const [key, value] of map) {
      expect(value.trim().length, key).toBeGreaterThan(0);
      expect(emoji.test(value), key).toBe(false);
    }
  });
});

describe("languages", () => {
  it("lists every translation once", () => {
    expect(LANGUAGES.map((l) => l.code)).toEqual(["en", "tr"]);
  });

  it("resolves settings and system languages", () => {
    expect(resolveLang("tr", "en-US")).toBe("tr");
    expect(resolveLang("system", "tr-TR")).toBe("tr");
    expect(resolveLang(undefined, "de-DE")).toBe("en");
  });
});
