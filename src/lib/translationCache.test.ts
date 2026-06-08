import { beforeEach, describe, expect, it } from "vitest";

import {
  getCachedSelectionTranslation,
  loadDocumentTranslationCache,
  makeSelectionTranslationKey,
  putCachedSelectionTranslation,
  type SelectionTranslationCacheInput,
} from "./translationCache";

const baseInput: SelectionTranslationCacheInput = {
  fileHash: "a".repeat(64),
  selectionText: "  Deep learning\n\npaper  ",
  sourceLanguage: "Auto",
  targetLanguage: "Chinese",
  promptProfileId: "academic-default",
  providerId: "openai-compatible",
};

describe("translation cache", () => {
  beforeEach(() => {
    installMemoryStorage();
  });

  it("returns cached translation for the same normalized selection", () => {
    putCachedSelectionTranslation(baseInput, "cached output");

    expect(
      getCachedSelectionTranslation({
        ...baseInput,
        selectionText: "Deep learning\n\npaper",
      }),
    ).toBe("cached output");
  });

  it("separates cache entries by target language", () => {
    putCachedSelectionTranslation(baseInput, "中文");

    expect(
      getCachedSelectionTranslation({
        ...baseInput,
        targetLanguage: "Japanese",
      }),
    ).toBeNull();
  });

  it("replaces a regenerated segment without duplicating it", () => {
    putCachedSelectionTranslation(baseInput, "first");
    putCachedSelectionTranslation(baseInput, "second");

    const cache = loadDocumentTranslationCache(baseInput.fileHash);
    expect(cache.segments).toHaveLength(1);
    expect(cache.segments[0].translatedText).toBe("second");
    expect(cache.segments[0].id).toBe(makeSelectionTranslationKey(baseInput));
  });
});

function installMemoryStorage() {
  const values = new Map<string, string>();
  const storage: Storage = {
    get length() {
      return values.size;
    },
    clear() {
      values.clear();
    },
    getItem(key: string) {
      return values.get(key) ?? null;
    },
    key(index: number) {
      return Array.from(values.keys())[index] ?? null;
    },
    removeItem(key: string) {
      values.delete(key);
    },
    setItem(key: string, value: string) {
      values.set(key, value);
    },
  };

  Object.defineProperty(globalThis, "localStorage", {
    value: storage,
    configurable: true,
  });
}
