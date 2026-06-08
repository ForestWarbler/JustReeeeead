import { normalizeSelectionText } from "$lib/selection";

export type FullDocumentCacheStatus = "idle" | "queued" | "running" | "complete" | "error";

export interface SelectionTranslationCacheInput {
  fileHash: string;
  selectionText: string;
  sourceLanguage: string;
  targetLanguage: string;
  promptProfileId: string;
  providerId: string;
}

export interface CachedTranslationSegment {
  id: string;
  sourceText: string;
  translatedText: string;
  sourceLanguage: string;
  targetLanguage: string;
  promptProfileId: string;
  providerId: string;
  updatedAt: string;
}

export interface DocumentTranslationCache {
  version: 1;
  fileHash: string;
  fullDocumentStatus: FullDocumentCacheStatus;
  segments: CachedTranslationSegment[];
}

const CACHE_PREFIX = "justreeeead.translationCache.v1.";
const MAX_SEGMENTS_PER_DOCUMENT = 2000;

export function loadDocumentTranslationCache(fileHash: string): DocumentTranslationCache {
  const storage = getStorage();
  if (!storage) {
    return createEmptyCache(fileHash);
  }

  try {
    const raw = storage.getItem(storageKey(fileHash));
    if (!raw) {
      return createEmptyCache(fileHash);
    }

    const parsed = JSON.parse(raw) as Partial<DocumentTranslationCache>;
    if (parsed.version !== 1 || parsed.fileHash !== fileHash || !Array.isArray(parsed.segments)) {
      return createEmptyCache(fileHash);
    }

    return {
      version: 1,
      fileHash,
      fullDocumentStatus: parsed.fullDocumentStatus ?? "idle",
      segments: parsed.segments.filter(isCacheSegment),
    };
  } catch {
    return createEmptyCache(fileHash);
  }
}

export function getCachedSelectionTranslation(input: SelectionTranslationCacheInput): string | null {
  const cache = loadDocumentTranslationCache(input.fileHash);
  const id = makeSelectionTranslationKey(input);
  return cache.segments.find((segment) => segment.id === id)?.translatedText ?? null;
}

export function putCachedSelectionTranslation(
  input: SelectionTranslationCacheInput,
  translatedText: string,
): DocumentTranslationCache {
  const cache = loadDocumentTranslationCache(input.fileHash);
  const id = makeSelectionTranslationKey(input);
  const sourceText = normalizeSelectionText(input.selectionText);
  const segment: CachedTranslationSegment = {
    id,
    sourceText,
    translatedText,
    sourceLanguage: input.sourceLanguage,
    targetLanguage: input.targetLanguage,
    promptProfileId: input.promptProfileId,
    providerId: input.providerId,
    updatedAt: new Date().toISOString(),
  };

  const next: DocumentTranslationCache = {
    ...cache,
    segments: [segment, ...cache.segments.filter((candidate) => candidate.id !== id)].slice(
      0,
      MAX_SEGMENTS_PER_DOCUMENT,
    ),
  };
  persistDocumentTranslationCache(next);
  return next;
}

export function getFullDocumentStatus(fileHash: string): FullDocumentCacheStatus {
  const cache = loadDocumentTranslationCache(fileHash);
  return cache.fullDocumentStatus;
}

export function updateFullDocumentStatus(
  fileHash: string,
  status: FullDocumentCacheStatus,
): DocumentTranslationCache {
  const cache = loadDocumentTranslationCache(fileHash);
  const next: DocumentTranslationCache = {
    ...cache,
    fullDocumentStatus: status,
  };
  persistDocumentTranslationCache(next);
  return next;
}

export function makeSelectionTranslationKey(input: SelectionTranslationCacheInput): string {
  return [
    "selection",
    input.fileHash,
    hashText(input.providerId),
    hashText(input.promptProfileId),
    hashText(input.sourceLanguage.toLowerCase()),
    hashText(input.targetLanguage.toLowerCase()),
    hashText(normalizeSelectionText(input.selectionText)),
  ].join(":");
}

export function hashText(value: string): string {
  let hash = 0x811c9dc5;
  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193);
  }
  return (hash >>> 0).toString(16).padStart(8, "0");
}

export function storageKeyForFileHash(fileHash: string): string {
  return storageKey(fileHash);
}

function persistDocumentTranslationCache(cache: DocumentTranslationCache) {
  getStorage()?.setItem(storageKey(cache.fileHash), JSON.stringify(cache));
}

function createEmptyCache(fileHash: string): DocumentTranslationCache {
  return {
    version: 1,
    fileHash,
    fullDocumentStatus: "idle",
    segments: [],
  };
}

function isCacheSegment(value: unknown): value is CachedTranslationSegment {
  if (!value || typeof value !== "object") {
    return false;
  }
  const segment = value as Partial<CachedTranslationSegment>;
  return (
    typeof segment.id === "string" &&
    typeof segment.sourceText === "string" &&
    typeof segment.translatedText === "string" &&
    typeof segment.sourceLanguage === "string" &&
    typeof segment.targetLanguage === "string" &&
    typeof segment.promptProfileId === "string" &&
    typeof segment.providerId === "string" &&
    typeof segment.updatedAt === "string"
  );
}

function storageKey(fileHash: string): string {
  return `${CACHE_PREFIX}${fileHash}`;
}

function getStorage(): Storage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}
