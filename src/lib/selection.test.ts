import { describe, expect, it } from "vitest";

import { normalizeSelectionText, selectionLooksUseful } from "./selection";

describe("selection text helpers", () => {
  it("normalizes spacing without flattening paragraphs", () => {
    expect(normalizeSelectionText("  Alpha\u00a0 beta  \n\n\n gamma\t\t delta ")).toBe(
      "Alpha beta\n\n gamma delta",
    );
  });

  it("rejects empty selections", () => {
    expect(selectionLooksUseful(" ")).toBe(false);
    expect(selectionLooksUseful("paper")).toBe(true);
  });
});
