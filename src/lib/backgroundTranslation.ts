import { listen } from "@tauri-apps/api/event";
import { getPageTextLayer, translateSelection } from "$lib/api";
import { normalizeSelectionText, selectionLooksUseful, splitIntoSentences } from "$lib/selection";
import {
  getCachedSelectionTranslation,
  putCachedSelectionTranslation,
  updateFullDocumentStatus,
  type SelectionTranslationCacheInput,
} from "$lib/translationCache";
import type {
  AppSettings,
  PageTextLayer,
  TranslationDelta,
  TranslationError,
  TranslationFinished,
} from "$lib/types";

const TEXT_EXTRACTION_IDLE_MS = 80;

interface TextChunk {
  text: string;
  pageIndex: number;
  blockId: number;
}

interface PendingChunk {
  cacheInput: SelectionTranslationCacheInput;
  text: string;
}

interface ActiveBackgroundJob {
  cacheInput: SelectionTranslationCacheInput;
  accumulatedOutput: string;
}

export type BackgroundProgressCallback = (completed: number, total: number) => void;

class BackgroundTranslationManager {
  private backgroundJobIds = new Set<string>();
  private activeJobs = new Map<string, ActiveBackgroundJob>();
  private queue: PendingChunk[] = [];
  private activeCount = 0;
  private totalChunks = 0;
  private completedChunks = 0;
  private fileHash: string | null = null;
  private cancelled = false;
  private concurrency = 2;
  private progressCallback: BackgroundProgressCallback | null = null;
  private settings: AppSettings | null = null;
  private unlistenFns: Array<() => void> = [];

  isBackgroundJob(jobId: string): boolean {
    return this.backgroundJobIds.has(jobId);
  }

  isRunning(): boolean {
    return this.activeCount > 0 || this.queue.length > 0;
  }

  async startFullDocumentTranslation(
    docId: string,
    fileHash: string,
    pageCount: number,
    settings: AppSettings,
    onProgress: BackgroundProgressCallback,
  ): Promise<void> {
    this.stop();

    this.fileHash = fileHash;
    this.settings = settings;
    this.cancelled = false;
    this.completedChunks = 0;
    this.totalChunks = 0;
    this.queue = [];
    this.progressCallback = onProgress;

    updateFullDocumentStatus(fileHash, "queued");

    await this.setupListeners();

    try {
      const chunks = await this.extractAllTextChunks(docId, pageCount);
      if (this.cancelled) return;

      const provider = settings.llmProviders[0];
      const prompt = settings.promptProfiles[0];
      if (!provider || !prompt) {
        updateFullDocumentStatus(fileHash, "error");
        return;
      }

      const pending: PendingChunk[] = [];
      for (const chunk of chunks) {
        const sentences = splitIntoSentences(chunk.text);
        for (const sentence of sentences) {
          const normalizedText = normalizeSelectionText(sentence);
          if (!selectionLooksUseful(normalizedText)) continue;

          const cacheInput: SelectionTranslationCacheInput = {
            fileHash,
            selectionText: normalizedText,
            sourceLanguage: prompt.sourceLanguage,
            targetLanguage: prompt.targetLanguage,
            promptProfileId: prompt.id,
            providerId: provider.id,
          };

          const cached = getCachedSelectionTranslation(cacheInput);
          if (cached === null) {
            pending.push({ cacheInput, text: normalizedText });
          }
        }
      }

      if (pending.length === 0) {
        updateFullDocumentStatus(fileHash, "complete");
        onProgress(0, 0);
        return;
      }

      this.totalChunks = pending.length;
      this.queue = pending;
      updateFullDocumentStatus(fileHash, "running");

      this.fillWorkerSlots();
    } catch (error) {
      console.error("Background translation failed:", error);
      updateFullDocumentStatus(fileHash, "error");
    }
  }

  stop() {
    this.cancelled = true;
    this.queue = [];
    this.activeJobs.clear();
    this.backgroundJobIds.clear();
    this.activeCount = 0;
    this.removeListeners();
  }

  private async setupListeners() {
    this.removeListeners();

    const unlistenDelta = await listen<TranslationDelta>(
      "translation_delta",
      (event) => {
        if (!this.backgroundJobIds.has(event.payload.jobId)) return;
        const job = this.activeJobs.get(event.payload.jobId);
        if (job) {
          job.accumulatedOutput += event.payload.delta;
        }
      },
    );

    const unlistenFinished = await listen<TranslationFinished>(
      "translation_finished",
      (event) => {
        if (!this.backgroundJobIds.has(event.payload.jobId)) return;

        const job = this.activeJobs.get(event.payload.jobId);
        if (job && !event.payload.cancelled && job.accumulatedOutput.trim()) {
          putCachedSelectionTranslation(job.cacheInput, job.accumulatedOutput);
        }

        this.finishJob(event.payload.jobId);
      },
    );

    const unlistenError = await listen<TranslationError>(
      "translation_error",
      (event) => {
        if (!this.backgroundJobIds.has(event.payload.jobId)) return;
        this.finishJob(event.payload.jobId);
      },
    );

    this.unlistenFns = [unlistenDelta, unlistenFinished, unlistenError];
  }

  private removeListeners() {
    for (const unlisten of this.unlistenFns) {
      unlisten();
    }
    this.unlistenFns = [];
  }

  private finishJob(jobId: string) {
    this.backgroundJobIds.delete(jobId);
    this.activeJobs.delete(jobId);
    this.activeCount--;
    this.completedChunks++;

    if (this.progressCallback) {
      this.progressCallback(this.completedChunks, this.totalChunks);
    }

    this.fillWorkerSlots();
    this.checkCompletion();
  }

  private fillWorkerSlots() {
    while (
      !this.cancelled &&
      this.activeCount < this.concurrency &&
      this.queue.length > 0
    ) {
      this.startNextChunk();
    }
  }

  private async startNextChunk() {
    const chunk = this.queue.shift();
    if (!chunk || !this.settings) return;

    this.activeCount++;

    const provider = this.settings.llmProviders[0];
    const prompt = this.settings.promptProfiles[0];
    if (!provider || !prompt) {
      this.activeCount--;
      this.completedChunks++;
      this.checkCompletion();
      return;
    }

    try {
      const job = await translateSelection({
        selectionText: chunk.text,
        sourceLanguage: prompt.sourceLanguage,
        targetLanguage: prompt.targetLanguage,
        promptProfileId: prompt.id,
        providerId: provider.id,
      });

      this.backgroundJobIds.add(job.jobId);
      this.activeJobs.set(job.jobId, {
        cacheInput: chunk.cacheInput,
        accumulatedOutput: "",
      });
    } catch (error) {
      console.error("Failed to start background translation job:", error);
      this.activeCount--;
      this.completedChunks++;
      this.fillWorkerSlots();
      this.checkCompletion();
    }
  }

  private checkCompletion() {
    if (
      this.activeCount === 0 &&
      this.queue.length === 0 &&
      this.fileHash &&
      !this.cancelled
    ) {
      updateFullDocumentStatus(this.fileHash, "complete");
    }
  }

  private async extractAllTextChunks(
    docId: string,
    pageCount: number,
  ): Promise<TextChunk[]> {
    const chunks: TextChunk[] = [];

    for (let pageIndex = 0; pageIndex < pageCount; pageIndex++) {
      if (this.cancelled) break;
      if (pageIndex > 0) {
        await sleep(TEXT_EXTRACTION_IDLE_MS);
      }

      try {
        const layer: PageTextLayer = await getPageTextLayer(docId, pageIndex);
        const lines = extractLinesFromLayer(layer);
        for (const line of lines) {
          chunks.push({
            text: line.text,
            pageIndex,
            blockId: line.blockId,
          });
        }
      } catch (error) {
        console.error(
          `Failed to extract text from page ${pageIndex}:`,
          error,
        );
      }
    }

    return chunks;
  }
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

function extractLinesFromLayer(
  layer: PageTextLayer,
): Array<{ blockId: number; text: string }> {
  type LineKey = `${number}:${number}`;
  const lineTexts = new Map<LineKey, string[]>();
  const meta = new Map<LineKey, { blockId: number }>();

  for (const span of layer.spans) {
    const k: LineKey = `${span.blockId}:${span.lineId}`;
    if (!lineTexts.has(k)) {
      lineTexts.set(k, []);
      meta.set(k, { blockId: span.blockId });
    }
    lineTexts.get(k)!.push(span.text);
  }

  const result: Array<{ blockId: number; text: string }> = [];
  for (const [k, chars] of lineTexts) {
    const text = chars.join("").trim();
    if (text.length > 0) {
      result.push({ blockId: meta.get(k)!.blockId, text });
    }
  }

  return result;
}

export const backgroundTranslation = new BackgroundTranslationManager();
