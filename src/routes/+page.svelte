<script lang="ts">
  import { onMount, tick } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { open } from "@tauri-apps/plugin-dialog";
  import {
    Bot,
    Dock,
    FolderOpen,
    KeyRound,
    Languages,
    Maximize2,
    Moon,
    PanelBottom,
    PanelLeft,
    PanelRight,
    Play,
    RefreshCw,
    RotateCw,
    Save,
    Settings as SettingsIcon,
    Square,
    Sun,
    ZoomIn,
    ZoomOut,
  } from "@lucide/svelte";

  import {
    cancelTranslation,
    getPageRenderUrl,
    getPageTextLayer,
    getSettings,
    openPdf,
    prefetchPageRenders,
    saveApiKey,
    saveSettings,
    translateSelection,
  } from "$lib/api";
  import { normalizeSelectionText, selectionLooksUseful } from "$lib/selection";
  import {
    getCachedSelectionTranslation,
    hashText,
    putCachedSelectionTranslation,
    type SelectionTranslationCacheInput,
    getFullDocumentStatus,
  } from "$lib/translationCache";
  import {
    appendTranslationDelta,
    beginTranslation,
    createTranslationState,
    failTranslation,
    finishTranslation,
  } from "$lib/translationState";
  import { backgroundTranslation } from "$lib/backgroundTranslation";
  import Sidebar from "$lib/Sidebar.svelte";
  import type {
    AppSettings,
    PageRenderRequest,
    PageTextLayer,
    PdfDocumentInfo,
    RenderQuality,
    TranslationDelta,
    TranslationError,
    TranslationFinished,
  } from "$lib/types";

  const PAGE_SIDE_PADDING = 56;
  const FIT_WIDTH_PADDING = 120;
  const MIN_ZOOM = 0.2;
  const MAX_ZOOM = 6;
  const ZOOM_STEP = 0.1;
  const TRANSLATION_DEBOUNCE_MS = 450;
  const RENDER_DEBOUNCE_MS = 500;
  const FINAL_PREFETCH_RADIUS = 1;
  const PREVIEW_PREFETCH_RADIUS = 3;

  interface SelectionHighlightRect {
    id: string;
    left: number;
    top: number;
    width: number;
    height: number;
  }

  let settings = $state<AppSettings | null>(null);
  let documentInfo = $state<PdfDocumentInfo | null>(null);
  let viewport = $state<HTMLDivElement | null>(null);
  let pageStage = $state<HTMLDivElement | null>(null);
  let scrollTop = $state(0);
  let viewportHeight = $state(0);
  let viewportWidth = $state(0);
  let displayZoom = $state(1);
  let targetZoom = 0;
  let renderUrls = $state<Record<string, string>>({});
  let lastRenderUrls = $state<Record<number, string>>({});
  let textLayers = $state<Record<number, PageTextLayer>>({});
  let selectedText = $state("");
  let renderZoom = $state(1);
  let isZooming = $state(false);
  let zoomStableTimer: ReturnType<typeof setTimeout> | null = null;
  let sidebarOpen = $state(false);
  let sidebarWidth = $state(260);
  let isResizingSidebar = false;
  let apiKeyInput = $state("");
  let settingsOpen = $state(false);
  let errorMessage = $state("");
  let translation = $state(createTranslationState());
  let backgroundProgress = $state<{ completed: number; total: number } | null>(null);
  let selectionHighlights = $state<SelectionHighlightRect[]>([]);
  let selectionHighlightFrame: number | null = null;
  let isSelectingText = false;
  let isResizingPanel = false;
  let translateTimer: ReturnType<typeof setTimeout> | null = null;
  let readerSaveTimer: ReturnType<typeof setTimeout> | null = null;
  let backgroundStartTimer: ReturnType<typeof setTimeout> | null = null;
  let lastSelectionKey = "";
  let activeTranslationCacheInput: SelectionTranslationCacheInput | null = null;
  let gestureStartZoom = 1;

  const loadingRenderUrls = new Set<string>();
  const queuedPrefetchKeys = new Set<string>();
  const loadingTextLayers = new Set<number>();

  const pageGap = $derived(settings?.reader.pageGap ?? 18);
  const rotation = $derived(settings?.reader.rotation ?? 0);
  const prefetchRadius = $derived(settings?.reader.prefetchRadius ?? 2);

  const pageTops = $derived.by(() => {
    if (!documentInfo) {
      return [];
    }

    let nextTop = pageGap;
    return documentInfo.pages.map((page) => {
      const top = nextTop;
      nextTop += pageDisplaySize(page.width, page.height).height + pageGap;
      return top;
    });
  });

  const totalHeight = $derived.by(() => {
    if (!documentInfo || pageTops.length === 0) {
      return 0;
    }
    const lastPage = documentInfo.pages[documentInfo.pages.length - 1];
    return pageTops[pageTops.length - 1] + pageDisplaySize(lastPage.width, lastPage.height).height + pageGap;
  });

  const viewportPageIndexes = $derived.by(() => {
    if (!documentInfo) {
      return [];
    }

    const topLimit = scrollTop - viewportHeight * 0.1;
    const bottomLimit = scrollTop + viewportHeight * 1.1;
    const indexes: number[] = [];

    for (let index = 0; index < documentInfo.pages.length; index += 1) {
      const page = documentInfo.pages[index];
      const top = pageTops[index] ?? 0;
      const bottom = top + pageDisplaySize(page.width, page.height).height;
      if (bottom >= topLimit && top <= bottomLimit) {
        indexes.push(index);
      }
    }

    return indexes;
  });

  const visiblePageIndexes = $derived.by(() => {
    if (!documentInfo) {
      return [];
    }

    const expanded = new Set<number>();
    const anchors = viewportPageIndexes.length > 0 ? viewportPageIndexes : [0];
    for (const index of anchors) {
      for (let offset = -FINAL_PREFETCH_RADIUS; offset <= FINAL_PREFETCH_RADIUS; offset += 1) {
        const candidate = index + offset;
        if (candidate >= 0 && candidate < documentInfo.pages.length) {
          expanded.add(candidate);
        }
      }
    }

    return [...expanded].sort((a, b) => a - b);
  });

  const prefetchPageIndexes = $derived.by(() => {
    if (!documentInfo) {
      return [];
    }

    const expanded = new Set<number>();
    const radius = Math.max(prefetchRadius, PREVIEW_PREFETCH_RADIUS);
    const anchors = viewportPageIndexes.length > 0 ? viewportPageIndexes : [0];
    for (const index of anchors) {
      for (let offset = -radius; offset <= radius; offset += 1) {
        const candidate = index + offset;
        if (candidate >= 0 && candidate < documentInfo.pages.length) {
          expanded.add(candidate);
        }
      }
    }

    return [...expanded].sort((a, b) => a - b);
  });

  const firstVisiblePage = $derived.by(() => {
    if (!documentInfo || viewportPageIndexes.length === 0) {
      return 0;
    }
    return Math.min(...viewportPageIndexes) + 1;
  });

  $effect(() => {
    if (!documentInfo || !settings) {
      return;
    }

    void renderZoom;
    void isZooming;
    for (const index of visiblePageIndexes) {
      void ensurePageAssets(index);
    }

    const prefetchRequests: PageRenderRequest[] = [];
    for (const index of prefetchPageIndexes) {
      prefetchRequests.push(buildRenderRequest(index, "preview"));
      if (!isZooming && distanceToViewport(index) <= FINAL_PREFETCH_RADIUS) {
        prefetchRequests.push(buildRenderRequest(index, "final"));
      }
    }
    void prefetchRenderRequests(prefetchRequests);
  });

  onMount(() => {
    updateViewportMetrics();
    window.addEventListener("resize", updateViewportMetrics);
    window.addEventListener("keydown", handleKeydown, { capture: true });
    document.addEventListener("selectionchange", handleDocumentSelectionChange);
    viewport?.addEventListener("gesturestart", handleGestureStart as EventListener, { passive: false });
    viewport?.addEventListener("gesturechange", handleGestureChange as EventListener, { passive: false });
    const cleanup: Array<() => void> = [];

    void (async () => {
      try {
        settings = await getSettings();
        displayZoom = settings?.reader.zoom ?? 1;
        renderZoom = settings?.reader.zoom ?? 1;
      } catch (error) {
        errorMessage = stringifyError(error);
      }

      cleanup.push(
        await listen<TranslationDelta>("translation_delta", (event) => {
          if (backgroundTranslation.isBackgroundJob(event.payload.jobId)) return;
          translation = appendTranslationDelta(translation, event.payload.jobId, event.payload.delta);
        }),
      );
      cleanup.push(
        await listen<TranslationFinished>("translation_finished", (event) => {
          if (backgroundTranslation.isBackgroundJob(event.payload.jobId)) return;
          const isActiveJob = translation.jobId === event.payload.jobId;
          if (isActiveJob) {
            if (!event.payload.cancelled && activeTranslationCacheInput && translation.output.trim()) {
              putCachedSelectionTranslation(activeTranslationCacheInput, translation.output);
            }
            activeTranslationCacheInput = null;
          }
          translation = finishTranslation(translation, event.payload.jobId);
        }),
      );
      cleanup.push(
        await listen<TranslationError>("translation_error", (event) => {
          if (backgroundTranslation.isBackgroundJob(event.payload.jobId)) return;
          if (translation.jobId === event.payload.jobId) {
            activeTranslationCacheInput = null;
          }
          translation = failTranslation(translation, event.payload.jobId, event.payload.message);
        }),
      );

      cleanup.push(
        await listen("menu-open-file", () => {
          void choosePdf();
        }),
      );

      cleanup.push(
        await listen<string>("menu-open-recent", (event) => {
          void openPdfByPath(event.payload);
        }),
      );
    })();

    return () => {
      window.removeEventListener("resize", updateViewportMetrics);
      window.removeEventListener("keydown", handleKeydown, { capture: true });
      document.removeEventListener("selectionchange", handleDocumentSelectionChange);
      viewport?.removeEventListener("gesturestart", handleGestureStart as EventListener);
      viewport?.removeEventListener("gesturechange", handleGestureChange as EventListener);
      backgroundTranslation.stop();
      clearTranslateTimer();
      clearReaderSaveTimer();
      clearBackgroundStartTimer();
      cancelSelectionHighlightRefresh();
      if (zoomStableTimer) {
        clearTimeout(zoomStableTimer);
        zoomStableTimer = null;
      }
      cleanup.forEach((unlisten) => unlisten());
    };
  });

  function pageDisplaySize(width: number, height: number) {
    const rotated = rotation % 180 !== 0;
    const z = displayZoom;
    return {
      width: (rotated ? height : width) * z,
      height: (rotated ? width : height) * z,
    };
  }

  function renderKey(pageIndex: number, quality: RenderQuality): string {
    return `${documentInfo?.docId ?? "none"}:${pageIndex}:${renderZoom.toFixed(3)}:${rotation}:${quality}`;
  }

  async function ensurePageAssets(pageIndex: number) {
    if (!documentInfo || !settings) {
      return;
    }

    void ensureRenderUrl(pageIndex, "preview");
    if (!isZooming && distanceToViewport(pageIndex) <= FINAL_PREFETCH_RADIUS) {
      void ensureRenderUrl(pageIndex, "final");
    }

    if (rotation !== 0 || isZooming || distanceToViewport(pageIndex) > FINAL_PREFETCH_RADIUS) {
      return;
    }

    if (!textLayers[pageIndex] && !loadingTextLayers.has(pageIndex)) {
      loadingTextLayers.add(pageIndex);
      try {
        const layer = await getPageTextLayer(documentInfo.docId, pageIndex);
        textLayers = { ...textLayers, [pageIndex]: layer };
      } catch {
        textLayers = { ...textLayers, [pageIndex]: { pageIndex, spans: [] } };
      } finally {
        loadingTextLayers.delete(pageIndex);
      }
    }
  }

  async function ensureRenderUrl(pageIndex: number, quality: RenderQuality) {
    if (!documentInfo) {
      return;
    }

    const key = renderKey(pageIndex, quality);
    if (renderUrls[key] || loadingRenderUrls.has(key)) {
      return;
    }

    const docId = documentInfo.docId;
    const requestRotation = rotation;
    loadingRenderUrls.add(key);
    try {
      const request = buildRenderRequest(pageIndex, quality);
      const url = await getPageRenderUrl(request);
      await preloadImage(url);
      if (documentInfo?.docId === docId && rotation === requestRotation) {
        renderUrls = { ...renderUrls, [key]: url };
        lastRenderUrls = { ...lastRenderUrls, [pageIndex]: url };
      }
    } catch (error) {
      errorMessage = stringifyError(error);
    } finally {
      loadingRenderUrls.delete(key);
    }
  }

  function bestRenderUrl(pageIndex: number): string {
    const finalUrl = renderUrls[renderKey(pageIndex, "final")];
    const previewUrl = renderUrls[renderKey(pageIndex, "preview")];
    return finalUrl ?? previewUrl ?? lastRenderUrls[pageIndex] ?? "";
  }

  function buildRenderRequest(pageIndex: number, quality: RenderQuality): PageRenderRequest {
    return {
      docId: documentInfo?.docId ?? "",
      pageIndex,
      zoom: renderZoom,
      dpr: quality === "preview" ? 1 : Math.min(window.devicePixelRatio || 1, 2),
      rotation,
      quality,
    };
  }

  function preloadImage(url: string): Promise<void> {
    return new Promise((resolve, reject) => {
      const image = new Image();
      image.decoding = "async";
      image.onload = () => resolve();
      image.onerror = () => reject(new Error("Failed to render page image"));
      image.src = url;
    });
  }

  function distanceToViewport(pageIndex: number): number {
    const anchors = viewportPageIndexes.length > 0 ? viewportPageIndexes : [0];
    return Math.min(...anchors.map((index) => Math.abs(index - pageIndex)));
  }

  async function prefetchRenderRequests(requests: PageRenderRequest[]) {
    if (!documentInfo || requests.length === 0) {
      return;
    }

    const uniqueRequests: PageRenderRequest[] = [];
    for (const request of requests) {
      const key = renderKey(request.pageIndex, request.quality);
      if (renderUrls[key] || loadingRenderUrls.has(key) || queuedPrefetchKeys.has(key)) {
        continue;
      }
      queuedPrefetchKeys.add(key);
      uniqueRequests.push(request);
    }

    if (uniqueRequests.length === 0) {
      return;
    }

    try {
      await prefetchPageRenders(uniqueRequests);
    } catch (error) {
      for (const request of uniqueRequests) {
        queuedPrefetchKeys.delete(renderKey(request.pageIndex, request.quality));
      }
      errorMessage = stringifyError(error);
    }
  }

  async function openDocument(path: string) {
    clearTranslateTimer();
    clearBackgroundStartTimer();
    lastSelectionKey = "";
    activeTranslationCacheInput = null;
    backgroundTranslation.stop();
    backgroundProgress = null;
    if (translation.jobId && translation.status !== "idle") {
      await cancelTranslation(translation.jobId);
    }
    documentInfo = await openPdf(path);
    renderUrls = {};
    lastRenderUrls = {};
    textLayers = {};
    queuedPrefetchKeys.clear();
    loadingRenderUrls.clear();
    loadingTextLayers.clear();
    selectedText = "";
    selectionHighlights = [];
    translation = createTranslationState();
    errorMessage = "";
    if (zoomStableTimer) {
      clearTimeout(zoomStableTimer);
      zoomStableTimer = null;
    }
    isZooming = false;
    targetZoom = 0;
    displayZoom = settings?.reader.zoom ?? 1;
    renderZoom = settings?.reader.zoom ?? 1;
    await tick();
    updateViewportMetrics();
    viewport?.scrollTo({ top: 0 });

    if (settings && documentInfo) {
      const openedDoc = documentInfo;
      const status = getFullDocumentStatus(openedDoc.fileHash);
      if (status !== "complete") {
        backgroundStartTimer = setTimeout(() => {
          backgroundStartTimer = null;
          if (!settings || documentInfo?.docId !== openedDoc.docId) {
            return;
          }
          void backgroundTranslation.startFullDocumentTranslation(
            openedDoc.docId,
            openedDoc.fileHash,
            openedDoc.pageCount,
            settings,
            (completed, total) => {
              backgroundProgress = { completed, total };
            },
          );
        }, 2500);
      }
    }
  }

  async function openPdfByPath(path: string) {
    try {
      await openDocument(path);
    } catch (error) {
      errorMessage = stringifyError(error);
    }
  }

  async function choosePdf() {
    const selected = await open({
      multiple: false,
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    });

    if (typeof selected !== "string") {
      return;
    }

    try {
      await openDocument(selected);
    } catch (error) {
      errorMessage = stringifyError(error);
    }
  }

  function onViewportScroll() {
    scrollTop = viewport?.scrollTop ?? 0;
    viewportHeight = viewport?.clientHeight ?? 0;
    viewportWidth = viewport?.clientWidth ?? 0;
  }

  function updateViewportMetrics() {
    viewportHeight = viewport?.clientHeight ?? window.innerHeight;
    viewportWidth = viewport?.clientWidth ?? window.innerWidth;
    scrollTop = viewport?.scrollTop ?? 0;
  }

  async function updateReader(next: Partial<AppSettings["reader"]>, persist: "immediate" | "debounced" = "immediate") {
    if (!settings) {
      return;
    }
    const oldRotation = settings.reader.rotation;
    const reader = {
      ...settings.reader,
      ...next,
      zoom: clampZoom(next.zoom ?? settings.reader.zoom),
      rotation: normalizeRotation(next.rotation ?? settings.reader.rotation),
    };
    settings = { ...settings, reader };

    if (reader.rotation !== oldRotation) {
      renderUrls = {};
      lastRenderUrls = {};
      queuedPrefetchKeys.clear();
      loadingRenderUrls.clear();
    }

    if (persist === "debounced") {
      scheduleReaderSave(reader);
      return;
    }

    clearReaderSaveTimer();
    settings = await saveSettings({ reader });
  }

  async function updateLayout(next: Partial<AppSettings["layout"]>) {
    if (!settings) {
      return;
    }
    const layout = { ...settings.layout, ...next };
    settings = { ...settings, layout };
    settings = await saveSettings({ layout });
  }

  async function fitWidth() {
    if (!documentInfo || !settings || !viewportWidth) {
      return;
    }
    const page = documentInfo.pages[0];
    const rotated = rotation % 180 !== 0;
    const baseWidth = rotated ? page.height : page.width;
    await setZoom((viewportWidth - FIT_WIDTH_PADDING) / baseWidth);
  }

  async function setZoom(
    nextZoom: number,
    persist: "immediate" | "debounced" = "immediate",
    anchor?: { clientX: number; clientY: number },
  ) {
    if (!settings) {
      return;
    }

    const clampedZoom = clampZoom(nextZoom);
    if (Math.abs(clampedZoom - displayZoom) < 0.0001) {
      return;
    }

    const prevZoom = displayZoom;
    displayZoom = clampedZoom;
    targetZoom = clampedZoom;
    selectionHighlights = [];

    if (zoomStableTimer) {
      clearTimeout(zoomStableTimer);
      zoomStableTimer = null;
    }

    const viewportElement = viewport;
    const rect = viewportElement?.getBoundingClientRect();
    const anchorX = anchor?.clientX ?? (rect ? rect.left + rect.width / 2 : window.innerWidth / 2);
    const anchorY = anchor?.clientY ?? (rect ? rect.top + rect.height / 2 : window.innerHeight / 2);
    const scrollAnchorX = viewportElement && rect ? viewportElement.scrollLeft + anchorX - rect.left : 0;
    const scrollAnchorY = viewportElement && rect ? viewportElement.scrollTop + anchorY - rect.top : 0;

    if (viewportElement && rect) {
      const ratio = clampedZoom / prevZoom;
      viewportElement.scrollTo({
        left: scrollAnchorX * ratio - (anchorX - rect.left),
        top: scrollAnchorY * ratio - (anchorY - rect.top),
      });
      updateViewportMetrics();
    }

    if (persist === "debounced") {
      isZooming = true;
      zoomStableTimer = setTimeout(() => {
        zoomStableTimer = null;
        void applyZoom("debounced");
      }, RENDER_DEBOUNCE_MS);
    } else {
      await applyZoom("immediate");
    }
  }

  async function applyZoom(persist: "immediate" | "debounced") {
    const zoomValue = targetZoom;
    if (!settings || zoomValue !== targetZoom) {
      return;
    }

    await updateReader({ zoom: zoomValue }, persist);
    if (zoomValue !== targetZoom) {
      return;
    }

    renderZoom = zoomValue;
    isZooming = false;
  }

  function scheduleReaderSave(reader: AppSettings["reader"]) {
    clearReaderSaveTimer();
    readerSaveTimer = setTimeout(() => {
      void (async () => {
        try {
          settings = await saveSettings({ reader });
        } catch (error) {
          errorMessage = stringifyError(error);
        }
      })();
    }, 280);
  }

  function clearReaderSaveTimer() {
    if (readerSaveTimer) {
      clearTimeout(readerSaveTimer);
      readerSaveTimer = null;
    }
  }

  function clearBackgroundStartTimer() {
    if (backgroundStartTimer) {
      clearTimeout(backgroundStartTimer);
      backgroundStartTimer = null;
    }
  }

  function clampZoom(value: number): number {
    return Number.isFinite(value) ? Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, value)) : 1;
  }

  function normalizeRotation(value: number): number {
    return ((Math.round(value / 90) * 90) % 360 + 360) % 360;
  }

  function handleKeydown(event: KeyboardEvent) {
    if (!(event.metaKey || event.ctrlKey) || isEditableTarget(event.target)) {
      return;
    }

    if (event.key === "+" || event.key === "=") {
      event.preventDefault();
      void setZoom(displayZoom + ZOOM_STEP);
    } else if (event.key === "-" || event.key === "_") {
      event.preventDefault();
      void setZoom(displayZoom - ZOOM_STEP);
    } else if (event.key === "0") {
      event.preventDefault();
      void setZoom(1);
    }
  }

  function handleViewportWheel(event: WheelEvent) {
    if (!(event.metaKey || event.ctrlKey)) {
      return;
    }

    event.preventDefault();
    const scale = Math.exp(-event.deltaY * 0.0003);
    void setZoom(displayZoom * scale, "debounced", { clientX: event.clientX, clientY: event.clientY });
  }

  function handleGestureStart(event: Event) {
    event.preventDefault();
    gestureStartZoom = displayZoom;
  }

  function handleGestureChange(event: Event) {
    event.preventDefault();
    const gestureEvent = event as Event & { scale?: number };
    const scale = Number.isFinite(gestureEvent.scale) ? gestureEvent.scale ?? 1 : 1;
    void setZoom(gestureStartZoom * scale, "debounced");
  }

  function isEditableTarget(target: EventTarget | null): boolean {
    const element = nodeElement(target);
    return Boolean(element?.closest("input, textarea, select, [contenteditable='true']"));
  }

  function handleReaderPointerDown(event: PointerEvent) {
    if (event.button !== 0 || !(event.target instanceof Node) || !viewport?.contains(event.target)) {
      return;
    }

    const targetElement = nodeElement(event.target);
    isSelectingText = Boolean(targetElement?.closest(".text-layer"));
    if (isSelectingText) {
      selectionHighlights = [];
      scheduleSelectionHighlightRefresh();
    }
  }

  function handleReaderPointerMove(event: PointerEvent) {
    if (!isSelectingText || !(event.target instanceof Node) || !viewport?.contains(event.target)) {
      return;
    }

    scheduleSelectionHighlightRefresh();
  }

  function handleReaderPointerUp(event: PointerEvent) {
    if (!(event.target instanceof Node) || !viewport?.contains(event.target)) {
      isSelectingText = false;
      return;
    }

    window.setTimeout(() => {
      refreshSelectionHighlights();
      updateSelectionFromReader();
      isSelectingText = false;
    }, 0);
  }

  function handleReaderPointerCancel() {
    isSelectingText = false;
  }

  function handleDocumentSelectionChange() {
    if (isSelectingText || selectionBelongsToReader()) {
      scheduleSelectionHighlightRefresh();
    }
  }

  function selectionBelongsToReader(): boolean {
    const selection = window.getSelection();
    if (!selection || selection.rangeCount === 0 || selection.isCollapsed || !viewport) {
      return false;
    }

    const range = selection.getRangeAt(0);
    return viewport.contains(range.commonAncestorContainer) && selectionTouchesTextLayer(range, selection);
  }

  function scheduleSelectionHighlightRefresh() {
    if (selectionHighlightFrame !== null) {
      return;
    }

    selectionHighlightFrame = window.requestAnimationFrame(() => {
      selectionHighlightFrame = null;
      refreshSelectionHighlights();
    });
  }

  function cancelSelectionHighlightRefresh() {
    if (selectionHighlightFrame !== null) {
      window.cancelAnimationFrame(selectionHighlightFrame);
      selectionHighlightFrame = null;
    }
  }

  function refreshSelectionHighlights() {
    const selection = getReaderSelection(true);
    if (selection) {
      selectionHighlights = selection.highlights;
      return;
    }

    if (isBrowserSelectionEmpty()) {
      selectionHighlights = [];
    }
  }

  function updateSelectionFromReader() {
    const selection = getReaderSelection();
    if (!selection) {
      if (isBrowserSelectionEmpty()) {
        lastSelectionKey = "";
        selectionHighlights = [];
      }
      return;
    }

    selectionHighlights = selection.highlights;
    if (selection.key === lastSelectionKey) {
      return;
    }

    lastSelectionKey = selection.key;
    selectedText = selection.text;
    activeTranslationCacheInput = null;
    clearTranslateTimer();

    if (translation.jobId && translation.status !== "idle") {
      void cancelTranslation(translation.jobId);
    }

    translation = {
      ...createTranslationState(),
      sourceText: selection.text,
    };

    if (!settings?.layout.translationOpen) {
      return;
    }

    scheduleSelectionTranslation(selection.text);
  }

  function getReaderSelection(preview = false): { text: string; key: string; highlights: SelectionHighlightRect[] } | null {
    const selection = window.getSelection();
    if (!selection || selection.rangeCount === 0 || selection.isCollapsed || !documentInfo || !viewport) {
      return null;
    }

    const range = selection.getRangeAt(0);
    if (!viewport.contains(range.commonAncestorContainer) || !selectionTouchesTextLayer(range, selection)) {
      return null;
    }

    const text = normalizeSelectionText(selection.toString());
    if (!preview && !selectionLooksUseful(text)) {
      return null;
    }

    const rects = Array.from(range.getClientRects()).filter((rect) => rect.width > 0 && rect.height > 0);
    if (rects.length === 0) {
      return null;
    }
    const highlights = buildSelectionHighlights(rects);
    if (highlights.length === 0) {
      return null;
    }

    const anchorSpan = nodeElement(selection.anchorNode)?.closest("[data-span-id]")?.getAttribute("data-span-id") ?? "";
    const focusSpan = nodeElement(selection.focusNode)?.closest("[data-span-id]")?.getAttribute("data-span-id") ?? "";
    const rectKey = rects
      .slice(0, 16)
      .map((rect) => `${Math.round(rect.left)},${Math.round(rect.top)},${Math.round(rect.right)},${Math.round(rect.bottom)}`)
      .join("|");
    const endpointKey = anchorSpan || focusSpan ? `${anchorSpan}:${focusSpan}` : rectKey;

    const orderedText = extractOrderedSelectionText(selection, text);
    const finalText = normalizeSelectionText(orderedText);
    if (!preview && !selectionLooksUseful(finalText)) {
      return null;
    }
    if (preview && finalText.length === 0) {
      return null;
    }

    return {
      text: finalText,
      key: `${documentInfo.fileHash}:${hashText(finalText)}:${endpointKey}`,
      highlights,
    };
  }

  function buildSelectionHighlights(rects: DOMRect[]): SelectionHighlightRect[] {
    if (!pageStage) {
      return [];
    }

    const stageRect = pageStage.getBoundingClientRect();
    const pageRects = rects
      .map((rect) => ({
        left: rect.left - stageRect.left,
        right: rect.right - stageRect.left,
        top: rect.top - stageRect.top,
        bottom: rect.bottom - stageRect.top,
        width: rect.width,
        height: rect.height,
        centerY: rect.top + rect.height / 2,
      }))
      .filter((rect) => rect.width > 0.6 && rect.height > 1.5)
      .sort((a, b) => a.centerY - b.centerY || a.left - b.left);

    const lines: Array<{
      centerY: number;
      height: number;
      rects: typeof pageRects;
    }> = [];

    for (const rect of pageRects) {
      const line = lines.find((candidate) => {
        const tolerance = Math.max(3, Math.min(candidate.height, rect.height) * 0.7);
        return Math.abs(candidate.centerY - rect.centerY) <= tolerance;
      });

      if (line) {
        line.rects.push(rect);
        line.centerY = median(line.rects.map((item) => item.centerY));
        line.height = median(line.rects.map((item) => item.height));
      } else {
        lines.push({
          centerY: rect.centerY,
          height: rect.height,
          rects: [rect],
        });
      }
    }

    return lines
      .flatMap((line, lineIndex) => {
        const sorted = [...line.rects].sort((a, b) => a.left - b.left);
        const groups: Array<typeof sorted> = [];

        for (const rect of sorted) {
          const current = groups[groups.length - 1];
          const currentRight = current ? Math.max(...current.map((item) => item.right)) : 0;
          const maxJoinGap = Math.max(24, line.height * 2.4);
          if (!current || rect.left - currentRight > maxJoinGap) {
            groups.push([rect]);
          } else {
            current.push(rect);
          }
        }

        return groups.map((group, groupIndex) => {
          const left = Math.min(...group.map((rect) => rect.left));
          const right = Math.max(...group.map((rect) => rect.right));
          const centerY = median(group.map((rect) => rect.top + rect.height / 2));
          const height = Math.max(8, median(group.map((rect) => rect.height)) * 1.08);
          const top = centerY - height / 2;

          return {
            id: `${lineIndex}-${groupIndex}`,
            left,
            top,
            width: right - left,
            height,
          };
        });
      })
      .filter((rect) => rect.width > 1 && rect.height > 1);
  }

  function median(values: number[]): number {
    if (values.length === 0) {
      return 0;
    }
    const sorted = [...values].sort((a, b) => a - b);
    const middle = Math.floor(sorted.length / 2);
    return sorted.length % 2 === 0 ? (sorted[middle - 1] + sorted[middle]) / 2 : sorted[middle];
  }

  function isBrowserSelectionEmpty(): boolean {
    const selection = window.getSelection();
    return !selection || selection.isCollapsed || normalizeSelectionText(selection.toString()).length === 0;
  }

  function extractOrderedSelectionText(selection: Selection, fallback: string): string {
    const range = selection.getRangeAt(0);
    const fragment = range.cloneContents();
    const walker = document.createTreeWalker(fragment, NodeFilter.SHOW_TEXT);
    const parts: string[] = [];
    let node: Text | null;
    while ((node = walker.nextNode() as Text | null)) {
      const parent = node.parentElement;
      if (parent?.hasAttribute("data-span-id")) {
        parts.push(node.textContent ?? "");
      }
    }
    const ordered = parts.join("");
    if (ordered.trim().length >= fallback.trim().length) {
      return ordered;
    }
    return fallback;
  }

  function selectionTouchesTextLayer(range: Range, selection: Selection): boolean {
    const anchorElement = nodeElement(selection.anchorNode);
    const focusElement = nodeElement(selection.focusNode);
    if (anchorElement?.closest(".text-layer") || focusElement?.closest(".text-layer")) {
      return true;
    }

    return Array.from(viewport?.querySelectorAll(".text-layer") ?? []).some((layer) => range.intersectsNode(layer));
  }

  function nodeElement(node: EventTarget | Node | null): Element | null {
    if (node instanceof Element) {
      return node;
    }
    return node instanceof Node ? node.parentElement : null;
  }

  function scheduleSelectionTranslation(text: string) {
    clearTranslateTimer();
    translateTimer = setTimeout(() => {
      void startTranslation(text);
    }, TRANSLATION_DEBOUNCE_MS);
  }

  function clearTranslateTimer() {
    if (translateTimer) {
      clearTimeout(translateTimer);
      translateTimer = null;
    }
  }

  function buildTranslationCacheInput(text: string): SelectionTranslationCacheInput | null {
    if (!documentInfo || !settings) {
      return null;
    }
    const provider = settings.llmProviders[0];
    const prompt = settings.promptProfiles[0];
    if (!provider || !prompt) {
      return null;
    }

    return {
      fileHash: documentInfo.fileHash,
      selectionText: text,
      sourceLanguage: prompt.sourceLanguage,
      targetLanguage: prompt.targetLanguage,
      promptProfileId: prompt.id,
      providerId: provider.id,
    };
  }

  async function startTranslation(text = selectedText, options: { force?: boolean } = {}) {
    const normalizedText = normalizeSelectionText(text);
    const cacheInput = buildTranslationCacheInput(normalizedText);
    if (!settings || !selectionLooksUseful(normalizedText) || !cacheInput) {
      return;
    }

    if (!options.force) {
      const cachedTranslation = getCachedSelectionTranslation(cacheInput);
      if (cachedTranslation !== null) {
        activeTranslationCacheInput = null;
        translation = {
          jobId: null,
          status: "idle",
          sourceText: normalizedText,
          output: cachedTranslation,
          error: "",
        };
        return;
      }
    }

    if (translation.jobId && translation.status !== "idle") {
      await cancelTranslation(translation.jobId);
    }

    const provider = settings.llmProviders[0];
    const prompt = settings.promptProfiles[0];
    activeTranslationCacheInput = cacheInput;
    translation = { ...translation, status: "loading", sourceText: normalizedText, output: "", error: "" };

    try {
      const job = await translateSelection({
        selectionText: normalizedText,
        sourceLanguage: prompt.sourceLanguage,
        targetLanguage: prompt.targetLanguage,
        promptProfileId: prompt.id,
        providerId: provider.id,
      });
      translation = beginTranslation(translation, job.jobId, normalizedText);
    } catch (error) {
      activeTranslationCacheInput = null;
      translation = {
        ...translation,
        status: "error",
        error: stringifyError(error),
      };
    }
  }

  async function regenerateTranslation() {
    await startTranslation(selectedText, { force: true });
  }

  async function stopTranslation() {
    if (!translation.jobId) {
      return;
    }
    activeTranslationCacheInput = null;
    await cancelTranslation(translation.jobId);
  }

  async function saveLlmSettings() {
    if (!settings) {
      return;
    }
    const provider = settings.llmProviders[0];
    const prompt = settings.promptProfiles[0];
    const nextProviders = [{ ...provider }];
    const nextPrompts = [{ ...prompt }];

    if (apiKeyInput.trim()) {
      await saveApiKey(provider.id, apiKeyInput.trim());
      apiKeyInput = "";
    }

    settings = await saveSettings({
      llmProviders: nextProviders,
      promptProfiles: nextPrompts,
    });
  }

  async function setProviderField(field: "baseUrl" | "model", value: string) {
    if (!settings) {
      return;
    }
    const [provider, ...rest] = settings.llmProviders;
    settings = {
      ...settings,
      llmProviders: [{ ...provider, [field]: value }, ...rest],
    };
  }

  async function setPromptField(field: "sourceLanguage" | "targetLanguage" | "promptTemplate", value: string) {
    if (!settings) {
      return;
    }
    const [prompt, ...rest] = settings.promptProfiles;
    settings = {
      ...settings,
      promptProfiles: [{ ...prompt, [field]: value }, ...rest],
    };
  }

  function beginPanelResize(event: MouseEvent) {
    if (!settings) {
      return;
    }

    isResizingPanel = true;
    event.preventDefault();
    window.addEventListener("mousemove", resizePanel);
    window.addEventListener("mouseup", endPanelResize, { once: true });
  }

  function resizePanel(event: MouseEvent) {
    if (!settings || !isResizingPanel) {
      return;
    }

    const nextSize =
      settings.layout.translationDock === "right"
        ? Math.max(280, Math.min(720, window.innerWidth - event.clientX))
        : Math.max(220, Math.min(560, window.innerHeight - event.clientY));
    settings = {
      ...settings,
      layout: { ...settings.layout, translationSize: nextSize },
    };
  }

  async function endPanelResize() {
    isResizingPanel = false;
    window.removeEventListener("mousemove", resizePanel);
    if (settings) {
      await updateLayout({ translationSize: settings.layout.translationSize });
    }
  }

  function beginSidebarResize(event: PointerEvent) {
    isResizingSidebar = true;
    const startX = event.clientX;
    const startWidth = sidebarWidth;

    function onMove(moveEvent: PointerEvent) {
      const next = Math.max(180, Math.min(480, startWidth + (moveEvent.clientX - startX)));
      sidebarWidth = next;
    }

    function onUp() {
      isResizingSidebar = false;
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
    }

    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp, { once: true });
  }

  function stringifyError(error: unknown): string {
    return error instanceof Error ? error.message : String(error);
  }
</script>

<svelte:head>
  <title>JustReeeeead</title>
</svelte:head>

<main
  class:dark-theme={settings?.layout.theme === "dark"}
  class:bottom-dock={settings?.layout.translationDock === "bottom"}
  class:panel-closed={!settings?.layout.translationOpen}
  class:sidebar-open={sidebarOpen}
  style:--panel-size={`${settings?.layout.translationSize ?? 380}px`}
  style:--sidebar-size={`${sidebarWidth}px`}
>
  <header class="toolbar">
    <div class="toolbar-group">
      <button class="icon-button primary" title="Open PDF" type="button" onclick={choosePdf}>
        <FolderOpen size={18} />
      </button>
      <button
        class="icon-button"
        class:active={sidebarOpen}
        title="Sidebar"
        type="button"
        onclick={() => (sidebarOpen = !sidebarOpen)}
      >
        <PanelLeft size={18} />
      </button>
      <div class="document-title">
        <strong>{documentInfo?.title ?? "JustReeeeead"}</strong>
        <span>{documentInfo ? `${firstVisiblePage} / ${documentInfo.pageCount}` : "No PDF open"}</span>
      </div>
      {#if backgroundProgress && backgroundProgress.total > 0}
        <div class="bg-translation-progress" title="Background translation: {backgroundProgress.completed} / {backgroundProgress.total} chunks">
          <progress value={backgroundProgress.completed} max={backgroundProgress.total}></progress>
          <span>{Math.round((backgroundProgress.completed / backgroundProgress.total) * 100)}%</span>
        </div>
      {/if}
    </div>

    <div class="toolbar-group center-tools">
      <button class="icon-button" title="Zoom out" type="button" onclick={() => setZoom(displayZoom - ZOOM_STEP)}>
        <ZoomOut size={18} />
      </button>
      <span class="metric">{Math.round(displayZoom * 100)}%</span>
      <button class="icon-button" title="Zoom in" type="button" onclick={() => setZoom(displayZoom + ZOOM_STEP)}>
        <ZoomIn size={18} />
      </button>
      <button class="icon-button" title="Fit width" type="button" onclick={fitWidth}>
        <Maximize2 size={18} />
      </button>
      <button class="icon-button" title="Rotate" type="button" onclick={() => updateReader({ rotation: rotation + 90 })}>
        <RotateCw size={18} />
      </button>
    </div>

    <div class="toolbar-group">
      <button
        class="icon-button"
        class:active={settings?.layout.translationOpen}
        title="Translation panel"
        type="button"
        onclick={() => updateLayout({ translationOpen: !settings?.layout.translationOpen })}
      >
        <Languages size={18} />
      </button>
      <button
        class="icon-button"
        title="Dock position"
        type="button"
        onclick={() =>
          updateLayout({
            translationDock: settings?.layout.translationDock === "right" ? "bottom" : "right",
          })}
      >
        {#if settings?.layout.translationDock === "bottom"}
          <PanelBottom size={18} />
        {:else}
          <PanelRight size={18} />
        {/if}
      </button>
      <button
        class="icon-button"
        class:active={settings?.layout.theme === "dark"}
        title="Theme"
        type="button"
        onclick={() =>
          updateLayout({
            theme: settings?.layout.theme === "dark" ? "light" : "dark",
          })}
      >
        {#if settings?.layout.theme === "dark"}
          <Sun size={18} />
        {:else}
          <Moon size={18} />
        {/if}
      </button>
      <button class="icon-button" class:active={settingsOpen} title="Settings" type="button" onclick={() => (settingsOpen = !settingsOpen)}>
        <SettingsIcon size={18} />
      </button>
    </div>
  </header>

  {#if settingsOpen && settings}
    <section class="settings-strip">
      <label>
        Base URL
        <input
          value={settings.llmProviders[0]?.baseUrl}
          oninput={(event) => setProviderField("baseUrl", event.currentTarget.value)}
        />
      </label>
      <label>
        Model
        <input
          value={settings.llmProviders[0]?.model}
          oninput={(event) => setProviderField("model", event.currentTarget.value)}
        />
      </label>
      <label>
        API Key
        <input type="password" bind:value={apiKeyInput} placeholder={settings.llmProviders[0]?.apiKeyConfigured ? "Configured" : "Required"} />
      </label>
      <label>
        From
        <input
          value={settings.promptProfiles[0]?.sourceLanguage}
          oninput={(event) => setPromptField("sourceLanguage", event.currentTarget.value)}
        />
      </label>
      <label>
        To
        <input
          value={settings.promptProfiles[0]?.targetLanguage}
          oninput={(event) => setPromptField("targetLanguage", event.currentTarget.value)}
        />
      </label>
      <button class="text-button" type="button" onclick={saveLlmSettings}>
        <Save size={16} />
        Save
      </button>
    </section>
  {/if}

  <section class="workspace">
    {#if sidebarOpen}
      <div class="sidebar-container">
        <Sidebar {documentInfo} onOpenDocument={openPdfByPath} />
        <div
          class="sidebar-resizer"
          role="separator"
          onpointerdown={beginSidebarResize}
        ></div>
      </div>
    {/if}
    <section class="reader-shell">
      {#if errorMessage}
        <div class="error-bar">{errorMessage}</div>
      {/if}

      {#if isZooming}
        <div class="zoom-indicator">{Math.round(displayZoom * 100)}%</div>
      {/if}

      <div
        bind:this={viewport}
        class="reader-viewport"
        role="region"
        aria-label="PDF reader"
        onscroll={onViewportScroll}
        onwheel={handleViewportWheel}
        onpointerdown={handleReaderPointerDown}
        onpointermove={handleReaderPointerMove}
        onpointerup={handleReaderPointerUp}
        onpointercancel={handleReaderPointerCancel}
      >
        {#if documentInfo}
          <div bind:this={pageStage} class="page-stage" style:height={`${totalHeight}px`}>
            {#each visiblePageIndexes as pageIndex (pageIndex)}
              {@const page = documentInfo.pages[pageIndex]}
              {@const size = pageDisplaySize(page.width, page.height)}
              {@const imageUrl = bestRenderUrl(pageIndex)}
              <article
                class="pdf-page"
                style:top={`${pageTops[pageIndex]}px`}
                style:width={`${size.width}px`}
                style:height={`${size.height}px`}
              >
                {#if imageUrl}
                  <img src={imageUrl} alt={`Page ${pageIndex + 1}`} draggable="false" />
                {:else}
                  <div class="page-loading">Rendering</div>
                {/if}

                {#if rotation === 0 && textLayers[pageIndex] && !isZooming}
                  <div class="text-layer" data-text-layer="true" style:width={`${size.width}px`} style:height={`${size.height}px`}>
                    {#each textLayers[pageIndex].spans as span (span.id)}
                      <span
                        class:line-break={span.text === "\n"}
                        data-span-id={span.id}
                        style:left={`${span.bbox.x * displayZoom}px`}
                        style:top={`${span.bbox.y * displayZoom}px`}
                        style:width={`${Math.max(1, span.bbox.width * displayZoom)}px`}
                        style:height={`${Math.max(1, span.bbox.height * displayZoom)}px`}
                        style:font-size={`${Math.max(1, span.fontSize * displayZoom)}px`}
                      >
                        {span.text}
                      </span>
                    {/each}
                  </div>
                {/if}
              </article>
            {/each}
            {#if selectionHighlights.length > 0 && !isZooming}
              <div class="selection-highlight-layer" aria-hidden="true">
                {#each selectionHighlights as rect (rect.id)}
                  <div
                    class="selection-highlight"
                    style:left={`${rect.left}px`}
                    style:top={`${rect.top}px`}
                    style:width={`${rect.width}px`}
                    style:height={`${rect.height}px`}
                  ></div>
                {/each}
              </div>
            {/if}
          </div>
        {:else}
          <div class="empty-reader">
            <button class="open-large" type="button" onclick={choosePdf}>
              <FolderOpen size={22} />
              Open PDF
            </button>
          </div>
        {/if}
      </div>
    </section>

    {#if settings?.layout.translationOpen}
      <aside class="translation-panel">
        <button class="resize-handle" type="button" title="Resize panel" onmousedown={beginPanelResize}>
          <Dock size={14} />
        </button>

        <div class="panel-header">
          <div>
            <strong>Translation</strong>
            <span>{settings.promptProfiles[0]?.sourceLanguage} -> {settings.promptProfiles[0]?.targetLanguage}</span>
          </div>
          <div class="panel-actions">
            <button class="icon-button" title="Translate selection" type="button" onclick={() => startTranslation()}>
              <Play size={17} />
            </button>
            <button class="icon-button" title="Regenerate selection" type="button" onclick={regenerateTranslation}>
              <RefreshCw size={17} />
            </button>
            <button class="icon-button" title="Stop" type="button" onclick={stopTranslation}>
              <Square size={17} />
            </button>
          </div>
        </div>

        <div class="translation-body">
          <section class="source-box">
            <div class="box-title">
              <KeyRound size={14} />
              Selection
            </div>
            <p>{selectedText || "Select text in the PDF."}</p>
          </section>

          <section class="output-box">
            <div class="box-title">
              <Bot size={14} />
              Result
              {#if translation.status === "loading" || translation.status === "streaming"}
                <span class="spin"><RefreshCw size={14} /></span>
              {/if}
            </div>
            {#if translation.error}
              <p class="translation-error">{translation.error}</p>
            {:else}
              <p>{translation.output || " "}</p>
            {/if}
          </section>

          {#if settingsOpen}
            <section class="prompt-editor">
              <label>
                Prompt
                <textarea
                  value={settings.promptProfiles[0]?.promptTemplate}
                  oninput={(event) => setPromptField("promptTemplate", event.currentTarget.value)}
                ></textarea>
              </label>
              <button class="text-button" type="button" onclick={saveLlmSettings}>
                <Save size={16} />
                Save Prompt
              </button>
            </section>
          {/if}
        </div>
      </aside>
    {/if}
  </section>
</main>

<style>
  :global(html, body) {
    margin: 0;
    width: 100%;
    height: 100%;
    overflow: hidden;
    font-family:
      ui-serif, Georgia, Cambria, "Times New Roman", Times, serif;
    color: #ebe6dc;
    background: #151515;
  }

  :global(button),
  :global(input),
  :global(textarea) {
    font: inherit;
  }

  main {
    --panel-size: 380px;
    --app-bg: #f4f4f1;
    --surface: #fbfbf8;
    --surface-raised: #ffffff;
    --surface-muted: #eeeeea;
    --reader-bg: #e2e2dd;
    --pdf-page-bg: #ffffff;
    --text: #262522;
    --muted: #68655f;
    --subtle: #827e76;
    --border: #d7d4cc;
    --border-strong: #c8c5bd;
    --accent: #4f7468;
    --accent-soft: rgba(79, 116, 104, 0.12);
    --accent-solid-text: #ffffff;
    --selection-bg: rgba(79, 116, 104, 0.24);
    --shadow: 0 18px 38px rgba(32, 31, 28, 0.12);
    --danger-bg: #fff4f0;
    --danger-border: #efb8aa;
    --danger-text: #963f2c;
    display: grid;
    grid-template-rows: 52px auto 1fr;
    width: 100vw;
    height: 100vh;
    min-width: 0;
    color: var(--text);
    background: var(--app-bg);
    color-scheme: light;
  }

  main.dark-theme {
    --app-bg: #151515;
    --surface: #1b1b1a;
    --surface-raised: #20201f;
    --surface-muted: #262625;
    --reader-bg: #171717;
    --pdf-page-bg: #fbfaf7;
    --text: #e9e4db;
    --muted: #aaa398;
    --subtle: #837d75;
    --border: #343331;
    --border-strong: #44423e;
    --accent: #9cb7a8;
    --accent-soft: rgba(156, 183, 168, 0.16);
    --accent-solid-text: #151515;
    --selection-bg: rgba(156, 183, 168, 0.27);
    --shadow: 0 18px 44px rgba(0, 0, 0, 0.34);
    --danger-bg: #2b1d1b;
    --danger-border: #6d3a31;
    --danger-text: #f0aaa0;
    color-scheme: dark;
  }

  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 14px;
    padding: 0 14px;
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }

  .toolbar-group {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .center-tools {
    flex: 0 0 auto;
  }

  .document-title {
    display: grid;
    min-width: 0;
    line-height: 1.15;
  }

  .document-title strong {
    max-width: 380px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 14px;
  }

  .document-title span,
  .panel-header span {
    color: var(--muted);
    font-size: 12px;
  }

  .bg-translation-progress {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 8px;
    border: 1px solid var(--border);
    border-radius: 7px;
    background: var(--surface-raised);
    font-size: 12px;
    color: var(--muted);
    white-space: nowrap;
  }

  .bg-translation-progress progress {
    width: 80px;
    height: 6px;
    border-radius: 3px;
    accent-color: var(--accent);
  }

  .bg-translation-progress progress::-webkit-progress-bar {
    background: var(--surface-muted);
    border-radius: 3px;
  }

  .bg-translation-progress progress::-webkit-progress-value {
    background: var(--accent);
    border-radius: 3px;
  }

  .icon-button,
  .text-button,
  .open-large {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 7px;
    border: 1px solid var(--border);
    background: var(--surface-raised);
    color: var(--text);
    cursor: pointer;
    transition:
      background 120ms ease,
      border-color 120ms ease,
      color 120ms ease;
  }

  .icon-button {
    width: 34px;
    height: 34px;
    border-radius: 7px;
  }

  .icon-button:hover,
  .text-button:hover,
  .open-large:hover,
  .icon-button.active {
    border-color: var(--accent);
    color: var(--accent);
    background: var(--accent-soft);
  }

  .icon-button.primary {
    color: var(--accent-solid-text);
    border-color: var(--accent);
    background: var(--accent);
  }

  .metric {
    min-width: 52px;
    color: var(--muted);
    font-size: 13px;
    text-align: center;
  }

  .settings-strip {
    display: grid;
    grid-template-columns: minmax(220px, 1.3fr) minmax(140px, 0.8fr) minmax(160px, 0.8fr) 100px 100px auto;
    gap: 10px;
    align-items: end;
    padding: 10px 14px;
    border-bottom: 1px solid var(--border);
    background: var(--surface-raised);
  }

  label {
    display: grid;
    gap: 5px;
    color: var(--muted);
    font-size: 12px;
  }

  input,
  textarea {
    box-sizing: border-box;
    width: 100%;
    border: 1px solid var(--border);
    border-radius: 7px;
    background: var(--surface);
    color: var(--text);
  }

  input {
    height: 34px;
    padding: 0 10px;
  }

  textarea {
    min-height: 132px;
    padding: 10px;
    resize: vertical;
    line-height: 1.45;
  }

  .text-button {
    height: 34px;
    padding: 0 12px;
    border-radius: 7px;
    white-space: nowrap;
  }

  .workspace {
    grid-row: 3;
    display: grid;
    grid-template-areas: "reader translation";
    grid-template-columns: minmax(0, 1fr) var(--panel-size);
    min-height: 0;
    min-width: 0;
  }

  main.sidebar-open .workspace {
    grid-template-areas: "sidebar reader translation";
    grid-template-columns: var(--sidebar-size) minmax(0, 1fr) var(--panel-size);
  }

  main.bottom-dock .workspace {
    grid-template-areas:
      "reader"
      "translation";
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: minmax(0, 1fr) var(--panel-size);
  }

  main.sidebar-open.bottom-dock .workspace {
    grid-template-areas:
      "sidebar reader"
      "sidebar translation";
    grid-template-columns: var(--sidebar-size) minmax(0, 1fr);
  }

  main.panel-closed .workspace {
    grid-template-areas: "reader";
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: minmax(0, 1fr);
  }

  main.sidebar-open.panel-closed .workspace {
    grid-template-areas: "sidebar reader";
    grid-template-columns: var(--sidebar-size) minmax(0, 1fr);
  }

  .sidebar-container {
    grid-area: sidebar;
    position: relative;
    min-width: 0;
    height: 100%;
    overflow: hidden;
    border-right: 1px solid var(--border);
    background: var(--surface);
  }

  .sidebar-container :global(.sidebar-panel) {
    width: 100%;
    height: 100%;
    box-sizing: border-box;
  }

  .sidebar-resizer {
    position: absolute;
    top: 0;
    right: -3px;
    bottom: 0;
    width: 7px;
    cursor: col-resize;
    z-index: 2;
    background: transparent;
  }

  .sidebar-resizer:hover {
    background: var(--accent);
    opacity: 0.3;
  }

  .reader-shell {
    grid-area: reader;
    position: relative;
    min-width: 0;
    min-height: 0;
    background: var(--reader-bg);
  }

  .reader-viewport {
    position: absolute;
    inset: 0;
    overflow: auto;
  }

  .page-stage {
    position: relative;
    min-width: 100%;
  }

  .selection-highlight-layer {
    position: absolute;
    inset: 0;
    z-index: 3;
    pointer-events: none;
  }

  .selection-highlight {
    position: absolute;
    border-radius: 2px;
    background: var(--selection-bg);
  }

  .pdf-page {
    position: absolute;
    left: 50%;
    overflow: hidden;
    background: var(--pdf-page-bg);
    border: 1px solid var(--border-strong);
    border-radius: 4px;
    box-shadow: var(--shadow);
    transform: translateX(-50%);
  }

  .pdf-page img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: fill;
    pointer-events: none;
    user-select: none;
  }

  .page-loading {
    display: grid;
    width: 100%;
    height: 100%;
    place-items: center;
    color: var(--subtle);
    font-size: 13px;
  }

  .text-layer {
    position: absolute;
    inset: 0;
    z-index: 1;
    overflow: hidden;
    pointer-events: auto;
    user-select: text;
    -webkit-user-select: text;
  }

  .text-layer span {
    position: absolute;
    display: block;
    color: transparent;
    line-height: 1;
    white-space: pre;
    user-select: text;
    -webkit-user-select: text;
  }

  .text-layer span.line-break {
    pointer-events: none;
  }

  .text-layer ::selection {
    background: transparent;
    color: transparent;
  }

  .empty-reader {
    display: grid;
    min-height: 100%;
    place-items: center;
  }

  .open-large {
    height: 42px;
    padding: 0 16px;
    border-radius: 8px;
    font-weight: 650;
  }

  .error-bar {
    position: absolute;
    top: 12px;
    left: 50%;
    z-index: 4;
    max-width: min(760px, calc(100% - 48px));
    padding: 9px 12px;
    border: 1px solid var(--danger-border);
    border-radius: 7px;
    background: var(--danger-bg);
    color: var(--danger-text);
    font-size: 13px;
    transform: translateX(-50%);
  }

  .zoom-indicator {
    position: absolute;
    top: 12px;
    right: 16px;
    z-index: 5;
    padding: 6px 12px;
    border-radius: 7px;
    background: var(--accent);
    color: var(--accent-solid-text);
    font-size: 14px;
    font-weight: 650;
    pointer-events: none;
    transition: opacity 200ms ease;
  }

  .translation-panel {
    grid-area: translation;
    position: relative;
    display: grid;
    grid-template-rows: auto 1fr;
    min-width: 0;
    min-height: 0;
    border-left: 1px solid var(--border);
    background: var(--surface);
  }

  main.bottom-dock .translation-panel {
    border-top: 1px solid var(--border);
    border-left: 0;
  }

  .resize-handle {
    position: absolute;
    top: 50%;
    left: -12px;
    z-index: 3;
    display: grid;
    width: 24px;
    height: 42px;
    place-items: center;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--surface-raised);
    color: var(--muted);
    cursor: ew-resize;
  }

  main.bottom-dock .resize-handle {
    top: -12px;
    left: 50%;
    width: 42px;
    height: 24px;
    cursor: ns-resize;
    transform: translateX(-50%);
  }

  .panel-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 14px;
    border-bottom: 1px solid var(--border);
  }

  .panel-header > div:first-child {
    display: grid;
    gap: 3px;
  }

  .panel-actions {
    display: flex;
    gap: 7px;
  }

  .translation-body {
    display: grid;
    align-content: start;
    gap: 12px;
    min-height: 0;
    overflow: auto;
    padding: 14px;
  }

  main.bottom-dock .translation-body {
    grid-template-columns: minmax(0, 0.9fr) minmax(0, 1.1fr);
  }

  .source-box,
  .output-box,
  .prompt-editor {
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--surface-raised);
  }

  .source-box,
  .output-box {
    min-height: 132px;
    padding: 12px;
    overflow: auto;
    min-width: 0;
  }

  .box-title {
    display: flex;
    align-items: center;
    gap: 7px;
    margin-bottom: 8px;
    color: var(--muted);
    font-size: 12px;
    font-weight: 700;
  }

  .source-box p,
  .output-box p {
    margin: 0;
    white-space: pre-wrap;
    overflow-wrap: break-word;
    word-break: break-word;
    line-height: 1.55;
    font-size: 14px;
  }

  .source-box p {
    color: var(--muted);
  }

  .translation-error {
    color: var(--danger-text);
  }

  .prompt-editor {
    display: grid;
    gap: 10px;
    padding: 12px;
  }

  .spin {
    display: inline-grid;
    width: 16px;
    height: 16px;
    place-items: center;
    margin-left: auto;
    line-height: 0;
  }

  .spin :global(svg) {
    display: block;
    animation: spin 900ms linear infinite;
    transform-box: fill-box;
    transform-origin: center;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  @media (max-width: 900px) {
    .settings-strip {
      grid-template-columns: 1fr 1fr;
    }

    .document-title strong {
      max-width: 220px;
    }

    .toolbar {
      gap: 8px;
    }
  }
</style>
