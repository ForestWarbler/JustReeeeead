import { describe, expect, it } from "vitest";

import { buildSearchPageIndex, findSearchMatches, normalizeSearchQuery } from "./search";
import type { PageTextLayer, TextSpan } from "./types";

describe("PDF search helpers", () => {
  it("normalizes case, compatibility characters, and whitespace", () => {
    expect(normalizeSearchQuery("  WORLD\nMODEL  ")).toBe("world model");
    expect(normalizeSearchQuery("ﬁeld")).toBe("field");
  });

  it("finds text across PDF line breaks by collapsing whitespace", () => {
    const index = buildSearchPageIndex(
      layer([
        span("world", 0, 0, 10, 10, 50),
        span("\n", 0, 0, 61, 10, 1),
        span("model", 0, 1, 10, 25, 45),
      ]),
    );

    const matches = findSearchMatches(index, "world model");

    expect(matches).toHaveLength(1);
    expect(matches[0].rects).toHaveLength(2);
  });

  it("estimates partial-word highlight bounds from character offsets", () => {
    const index = buildSearchPageIndex(layer([span("prediction", 0, 0, 10, 10, 100)]));

    const [match] = findSearchMatches(index, "dict");

    expect(match.rects).toHaveLength(1);
    expect(match.rects[0].left).toBeCloseTo(40);
    expect(match.rects[0].width).toBeCloseTo(40);
  });

  it("returns non-overlapping matches in reading order", () => {
    const index = buildSearchPageIndex(layer([span("agent agent agent", 0, 0, 10, 10, 150)]));

    const matches = findSearchMatches(index, "agent");

    expect(matches.map((match) => match.start)).toEqual([0, 6, 12]);
  });
});

function layer(spans: TextSpan[]): PageTextLayer {
  return { pageIndex: 0, spans };
}

function span(text: string, blockId: number, lineId: number, x: number, y: number, width: number): TextSpan {
  return {
    id: `${blockId}-${lineId}-${text}-${x}`,
    text,
    bbox: { x, y, width, height: 10 },
    quad: {
      x1: x,
      y1: y,
      x2: x + width,
      y2: y,
      x3: x + width,
      y3: y + 10,
      x4: x,
      y4: y + 10,
    },
    fontSize: 10,
    blockId,
    lineId,
  };
}
