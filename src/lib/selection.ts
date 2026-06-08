export function normalizeSelectionText(value: string): string {
  return value
    .replace(/\u00a0/g, " ")
    .replace(/[ \t]+\n/g, "\n")
    .replace(/\n{3,}/g, "\n\n")
    .replace(/[ \t]{2,}/g, " ")
    .trim();
}

export function selectionLooksUseful(value: string): boolean {
  return normalizeSelectionText(value).length >= 2;
}

export function splitIntoSentences(text: string): string[] {
  const MAX_SENTENCE_LEN = 300;
  const raw: string[] = [];
  let current = "";
  for (let i = 0; i < text.length; i++) {
    const ch = text[i];
    if (ch === "\n") {
      if (current.trim().length > 0) {
        raw.push(current.trim());
      }
      current = "";
      continue;
    }
    current += ch;
    const isTerminator = ".!?。！？".includes(ch);
    if (isTerminator) {
      const next = i + 1 < text.length ? text[i + 1] : "";
      const isQuoteOrParen = "\"'」』）)】]".includes(next);
      if (!isQuoteOrParen) {
        raw.push(current.trim());
        current = "";
      }
    } else if (current.length > MAX_SENTENCE_LEN) {
      const breakAt = current.lastIndexOf(" ");
      if (breakAt > MAX_SENTENCE_LEN * 0.5) {
        raw.push(current.slice(0, breakAt).trim());
        current = current.slice(breakAt + 1);
      } else {
        raw.push(current.trim());
        current = "";
      }
    }
  }
  const remainder = current.trim();
  if (remainder.length > 0) {
    raw.push(remainder);
  }
  return raw.filter((s) => s.length >= 2);
}
