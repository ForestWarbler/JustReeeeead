import type { PageTextLayer, RectDto, TextSpan } from "$lib/types";

export interface SearchPageIndex {
  pageIndex: number;
  normalizedText: string;
  charMap: SearchCharMap[];
  spans: TextSpan[];
}

export interface SearchCharMap {
  spanIndex: number;
  charIndex: number;
}

export interface SearchMatch {
  id: string;
  pageIndex: number;
  start: number;
  end: number;
  rects: SearchRect[];
}

export interface SearchRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

const MAX_MATCHES_PER_PAGE = 1_000;

export function normalizeSearchQuery(value: string): string {
  const normalized: string[] = [];
  let previousWasSpace = true;

  for (const rawChar of Array.from(value.normalize("NFKC"))) {
    const folded = rawChar.toLocaleLowerCase();
    for (const char of Array.from(folded)) {
      if (isSearchWhitespace(char)) {
        if (!previousWasSpace) {
          normalized.push(" ");
          previousWasSpace = true;
        }
        continue;
      }

      normalized.push(char);
      previousWasSpace = false;
    }
  }

  while (normalized[normalized.length - 1] === " ") {
    normalized.pop();
  }

  return normalized.join("");
}

export function buildSearchPageIndex(layer: PageTextLayer): SearchPageIndex {
  const normalized: string[] = [];
  const charMap: SearchCharMap[] = [];
  let previousWasSpace = true;

  layer.spans.forEach((span, spanIndex) => {
    Array.from(span.text.normalize("NFKC")).forEach((rawChar, charIndex) => {
      const folded = rawChar.toLocaleLowerCase();
      for (const char of Array.from(folded)) {
        if (isSearchWhitespace(char)) {
          if (!previousWasSpace) {
            normalized.push(" ");
            charMap.push({ spanIndex, charIndex });
            previousWasSpace = true;
          }
          continue;
        }

        normalized.push(char);
        charMap.push({ spanIndex, charIndex });
        previousWasSpace = false;
      }
    });
  });

  while (normalized[normalized.length - 1] === " ") {
    normalized.pop();
    charMap.pop();
  }

  return {
    pageIndex: layer.pageIndex,
    normalizedText: normalized.join(""),
    charMap,
    spans: layer.spans,
  };
}

export function findSearchMatches(index: SearchPageIndex, rawQuery: string): SearchMatch[] {
  const query = normalizeSearchQuery(rawQuery);
  if (!query) {
    return [];
  }

  const matches: SearchMatch[] = [];
  let fromIndex = 0;

  while (fromIndex < index.normalizedText.length && matches.length < MAX_MATCHES_PER_PAGE) {
    const start = index.normalizedText.indexOf(query, fromIndex);
    if (start === -1) {
      break;
    }

    const end = start + query.length;
    const rects = buildSearchMatchRects(index, start, end);
    if (rects.length > 0) {
      matches.push({
        id: `${index.pageIndex}:${start}:${end}`,
        pageIndex: index.pageIndex,
        start,
        end,
        rects,
      });
    }

    fromIndex = end;
  }

  return matches;
}

function buildSearchMatchRects(index: SearchPageIndex, start: number, end: number): SearchRect[] {
  const rangesBySpan = new Map<number, { start: number; end: number }>();

  for (let offset = start; offset < end; offset += 1) {
    const mapped = index.charMap[offset];
    const span = mapped ? index.spans[mapped.spanIndex] : null;
    if (!mapped || !span || isBlankSpan(span)) {
      continue;
    }

    const range = rangesBySpan.get(mapped.spanIndex);
    if (range) {
      range.start = Math.min(range.start, mapped.charIndex);
      range.end = Math.max(range.end, mapped.charIndex + 1);
    } else {
      rangesBySpan.set(mapped.spanIndex, {
        start: mapped.charIndex,
        end: mapped.charIndex + 1,
      });
    }
  }

  const lines = new Map<string, RectDto[]>();
  for (const [spanIndex, range] of rangesBySpan) {
    const span = index.spans[spanIndex];
    const charCount = Math.max(1, Array.from(span.text.normalize("NFKC")).length);
    const startRatio = Math.max(0, Math.min(1, range.start / charCount));
    const endRatio = Math.max(startRatio, Math.min(1, range.end / charCount));
    const left = span.bbox.x + span.bbox.width * startRatio;
    const right = span.bbox.x + span.bbox.width * endRatio;
    const rect = {
      x: left,
      y: span.bbox.y,
      width: Math.max(1, right - left),
      height: span.bbox.height,
    };
    const key = `${span.blockId}:${span.lineId}`;
    lines.set(key, [...(lines.get(key) ?? []), rect]);
  }

  return [...lines.values()]
    .map((rects) => {
      const left = Math.min(...rects.map((rect) => rect.x));
      const right = Math.max(...rects.map((rect) => rect.x + rect.width));
      const top = Math.min(...rects.map((rect) => rect.y));
      const bottom = Math.max(...rects.map((rect) => rect.y + rect.height));
      return {
        left,
        top,
        width: right - left,
        height: Math.max(2, bottom - top),
      };
    })
    .filter((rect) => rect.width > 0 && rect.height > 0)
    .sort((a, b) => a.top - b.top || a.left - b.left);
}

function isSearchWhitespace(char: string): boolean {
  return /\s|\u00a0/.test(char);
}

function isBlankSpan(span: TextSpan): boolean {
  return normalizeSearchQuery(span.text).length === 0;
}
