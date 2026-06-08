<script lang="ts">
  import { onMount } from "svelte";
  import {
    BookOpen,
    FolderOpen,
    FolderPlus,
    Library,
    Trash2,
    ChevronRight,
    FileText,
    Plus,
    X,
  } from "@lucide/svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import {
    getLibrary,
    addLibraryFolder,
    removeLibraryFolder,
    addLibraryDocument,
    removeLibraryDocument,
    getRecentFiles,
    openPdf,
    closePdf,
  } from "$lib/api";
  import type { LibraryData, LibraryFolder, LibraryDocument, PdfDocumentInfo, RecentFileEntry, PdfOutlineItem } from "$lib/types";

  interface Props {
    onOpenDocument: (path: string) => Promise<void>;
    documentInfo: PdfDocumentInfo | null;
  }

  let { onOpenDocument, documentInfo }: Props = $props();

  let libraryData = $state<LibraryData>({ folders: [], documents: [] });
  let recentFiles = $state<RecentFileEntry[]>([]);
  let newFolderName = $state("");
  let showNewFolderInput = $state(false);
  let selectedFolderId = $state<string | null>(null);
  let selectedDocId = $state<string | null>(null);
  let errorMessage = $state("");

  let foldersExpanded = $state(true);
  let recentExpanded = $state(true);
  let outlineExpanded = $state(true);

  onMount(() => {
    loadLibrary();
    loadRecentFiles();
  });

  async function loadLibrary() {
    try {
      libraryData = await getLibrary();
    } catch (e) {
      console.error("Failed to load library:", e);
    }
  }

  async function loadRecentFiles() {
    try {
      recentFiles = await getRecentFiles();
    } catch (e) {
      console.error("Failed to load recent files:", e);
    }
  }

  function rootDocuments(): LibraryDocument[] {
    return libraryData.documents.filter((d) => d.folderId === null);
  }

  function folderDocuments(folderId: string): LibraryDocument[] {
    return libraryData.documents.filter((d) => d.folderId === folderId);
  }

  async function handleCreateFolder() {
    const name = newFolderName.trim();
    if (!name) return;
    try {
      libraryData = await addLibraryFolder(name);
      newFolderName = "";
      showNewFolderInput = false;
    } catch (e) {
      errorMessage = String(e);
    }
  }

  async function handleDeleteFolder(folderId: string) {
    try {
      libraryData = await removeLibraryFolder(folderId);
      if (selectedFolderId === folderId) selectedFolderId = null;
    } catch (e) {
      errorMessage = String(e);
    }
  }

  async function handleAddDocument(folderId: string | null = null) {
    try {
      const selected = await open({
        multiple: false,
        filters: [{ name: "PDF", extensions: ["pdf"] }],
      });
      if (typeof selected !== "string") return;

      const info = await openPdf(selected);
      libraryData = await addLibraryDocument(
        info.path,
        info.title,
        info.fileHash,
        folderId,
      );
      await closePdf(info.docId);
    } catch (e) {
      errorMessage = String(e);
    }
  }

  async function handleRemoveDocument(docId: string) {
    try {
      libraryData = await removeLibraryDocument(docId);
    } catch (e) {
      errorMessage = String(e);
    }
  }

  async function handleOpenLibraryDocument(doc: LibraryDocument) {
    selectedDocId = doc.id;
    await loadRecentFiles();
    await onOpenDocument(doc.path);
  }

  async function handleOpenRecent(file: RecentFileEntry) {
    await onOpenDocument(file.path);
  }

  function flattenOutline(
    items: PdfOutlineItem[],
    depth: number = 0,
  ): Array<{ item: PdfOutlineItem; depth: number }> {
    const result: Array<{ item: PdfOutlineItem; depth: number }> = [];
    for (const item of items) {
      result.push({ item, depth });
      result.push(...flattenOutline(item.children, depth + 1));
    }
    return result;
  }

  $effect(() => {
    if (documentInfo) {
      outlineItems = flattenOutline(documentInfo.outline);
    } else {
      outlineItems = [];
    }
  });

  let outlineItems = $state<Array<{ item: PdfOutlineItem; depth: number }>>([]);

  function handleNavigateToPage(pageIndex: number) {
    if (!documentInfo) return;
    const tops = calculatePageTops();
    const top = tops[pageIndex] ?? 0;
    const viewport = document.querySelector(".reader-viewport");
    if (viewport) {
      viewport.scrollTo({ top: top - 80, behavior: "smooth" });
    }
  }

  function calculatePageTops(): number[] {
    if (!documentInfo) return [];
    const gap = 18;
    let top = gap;
    return documentInfo.pages.map((page) => {
      const t = top;
      top += page.height + gap;
      return t;
    });
  }

  function isDocumentActive(path: string): boolean {
    return documentInfo?.path === path;
  }

  function truncate(text: string, max: number): string {
    return text.length > max ? text.slice(0, max) + "…" : text;
  }
</script>

<div class="sidebar-panel">
  <div class="sidebar-top">
    <div>
      <span>Workspace</span>
      <strong>Documents</strong>
    </div>
    <button class="icon-btn-sm prominent" title="Add PDF" onclick={() => handleAddDocument(selectedFolderId)}>
      <Plus size={14} />
    </button>
  </div>

  {#if errorMessage}
    <div class="sidebar-error">{errorMessage}</div>
  {/if}

  <!-- Chapters / Outline -->
  {#if documentInfo}
    <div class="sidebar-section">
      <button class="sidebar-section-header" onclick={() => (outlineExpanded = !outlineExpanded)}>
        <span class="chevron" class:rotated={outlineExpanded}><ChevronRight size={12} /></span>
        <BookOpen size={14} />
        <span>Chapters</span>
        <span class="section-count">{outlineItems.length}</span>
      </button>
      {#if outlineExpanded}
        {#if outlineItems.length > 0}
          <div class="outline-tree">
            {#each outlineItems as entry}
              <button
                class="outline-item"
                style="padding-left: {12 + entry.depth * 14}px"
                title={entry.item.title}
                onclick={() => handleNavigateToPage(entry.item.page)}
              >
                <span>{entry.item.title}</span>
                <span class="outline-page">{entry.item.page + 1}</span>
              </button>
            {/each}
          </div>
        {:else}
          <div class="sidebar-empty">No chapter outline in this PDF.</div>
        {/if}
      {/if}
    </div>
  {/if}

  <!-- Recent Files -->
  {#if recentFiles.length > 0}
    <div class="sidebar-section">
      <button class="sidebar-section-header" onclick={() => (recentExpanded = !recentExpanded)}>
        <span class="chevron" class:rotated={recentExpanded}><ChevronRight size={12} /></span>
        <FileText size={14} />
        <span>Recent</span>
        <span class="section-count">{recentFiles.length}</span>
      </button>
      {#if recentExpanded}
        <div class="recent-list">
          {#each recentFiles as file}
            <div
              class="doc-item"
              class:active={isDocumentActive(file.path)}
              role="button"
              tabindex="0"
              onclick={() => handleOpenRecent(file)}
              onkeydown={(e) => e.key === 'Enter' && handleOpenRecent(file)}
              title={file.path}
            >
              <FileText size={13} />
              <span>{truncate(file.title, 40)}</span>
            </div>
          {/each}
        </div>
      {/if}
    </div>
  {/if}

  <!-- Library -->
  <div class="sidebar-section">
    <div class="sidebar-section-header-row">
      <button class="sidebar-section-header" onclick={() => (foldersExpanded = !foldersExpanded)}>
        <span class="chevron" class:rotated={foldersExpanded}><ChevronRight size={12} /></span>
        <Library size={14} />
        <span>Library</span>
        <span class="section-count">{libraryData.folders.length + rootDocuments().length}</span>
      </button>
      <div class="section-actions">
        <button class="icon-btn-sm" title="Add folder" onclick={() => (showNewFolderInput = true)}>
          <FolderPlus size={14} />
        </button>
        <button class="icon-btn-sm" title="Add PDF" onclick={() => handleAddDocument(selectedFolderId)}>
          <Plus size={14} />
        </button>
      </div>
    </div>

    {#if showNewFolderInput}
      <div class="new-folder-row">
        <input
          type="text"
          placeholder="Folder name"
          bind:value={newFolderName}
          onkeydown={(e) => e.key === "Enter" && handleCreateFolder()}
        />
        <button class="icon-btn-sm" onclick={handleCreateFolder}><Plus size={14} /></button>
        <button class="icon-btn-sm" onclick={() => { showNewFolderInput = false; newFolderName = ''; }}><X size={14} /></button>
      </div>
    {/if}

    {#if foldersExpanded}
      <!-- Root documents -->
      {#each rootDocuments() as doc (doc.id)}
        <div
          class="doc-item"
          class:active={isDocumentActive(doc.path)}
          role="button"
          tabindex="0"
          onclick={() => handleOpenLibraryDocument(doc)}
          onkeydown={(e) => e.key === 'Enter' && handleOpenLibraryDocument(doc)}
          title={doc.path}
        >
          <FileText size={13} />
          <span>{truncate(doc.title, 40)}</span>
          <button class="icon-btn-sm delete-btn" onclick={(e) => { e.stopPropagation(); handleRemoveDocument(doc.id); }}>
            <Trash2 size={12} />
          </button>
        </div>
      {/each}

      <!-- Folders -->
      {#each libraryData.folders as folder (folder.id)}
        <div class="folder-group">
          <div
            class="folder-header"
            class:selected={selectedFolderId === folder.id}
            role="button"
            tabindex="0"
            onclick={() => selectedFolderId = selectedFolderId === folder.id ? null : folder.id}
            onkeydown={(e) => e.key === 'Enter' && (selectedFolderId = selectedFolderId === folder.id ? null : folder.id)}
          >
            <span class="chevron" class:rotated={selectedFolderId === folder.id}><ChevronRight size={12} /></span>
            <FolderOpen size={13} />
            <span>{truncate(folder.name, 30)}</span>
            <span class="folder-count">{folderDocuments(folder.id).length}</span>
            <button class="icon-btn-sm delete-btn" onclick={(e) => { e.stopPropagation(); handleDeleteFolder(folder.id); }}>
              <Trash2 size={12} />
            </button>
          </div>
          {#if selectedFolderId === folder.id}
            <div class="folder-docs">
              <button class="add-to-folder-btn" onclick={() => handleAddDocument(folder.id)}>
                <Plus size={12} /> Add PDF
              </button>
              {#each folderDocuments(folder.id) as doc (doc.id)}
                <div
                  class="doc-item nested"
                  class:active={isDocumentActive(doc.path)}
                  role="button"
                  tabindex="0"
                  onclick={() => handleOpenLibraryDocument(doc)}
                  onkeydown={(e) => e.key === 'Enter' && handleOpenLibraryDocument(doc)}
                  title={doc.path}
                >
                  <FileText size={13} />
                  <span>{truncate(doc.title, 38)}</span>
                  <button class="icon-btn-sm delete-btn" onclick={(e) => { e.stopPropagation(); handleRemoveDocument(doc.id); }}>
                    <Trash2 size={12} />
                  </button>
                </div>
              {/each}
            </div>
          {/if}
        </div>
      {/each}
    {/if}
  </div>
</div>

<style>
  .sidebar-panel {
    display: flex;
    flex-direction: column;
    min-height: 0;
    overflow-y: auto;
    padding: 14px 12px;
    gap: 12px;
    background: var(--surface);
    font-size: 13px;
    user-select: none;
  }

  .sidebar-top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 0 2px 10px;
    border-bottom: 1px solid var(--border);
  }

  .sidebar-top > div {
    display: grid;
    gap: 2px;
    min-width: 0;
  }

  .sidebar-top span {
    color: var(--subtle);
    font-size: 11px;
  }

  .sidebar-top strong {
    overflow: hidden;
    color: var(--text);
    font-size: 17px;
    font-weight: 650;
    line-height: 1.1;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .sidebar-error {
    padding: 8px 10px;
    border: 1px solid var(--danger-border);
    border-radius: 7px;
    background: var(--danger-bg);
    color: var(--danger-text);
    font-size: 12px;
  }

  .sidebar-section {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }

  .sidebar-section-header-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .section-actions {
    display: flex;
    gap: 4px;
  }

  .sidebar-section-header {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    min-width: 0;
    padding: 6px 4px;
    border: none;
    background: none;
    color: var(--muted);
    font-size: 12px;
    font-weight: 650;
    cursor: pointer;
    border-radius: 6px;
    text-transform: uppercase;
    letter-spacing: 0;
  }

  .sidebar-section-header:hover {
    background: var(--surface-muted);
  }

  .sidebar-section-header .chevron {
    display: inline-flex;
    transition: transform 150ms ease;
    flex-shrink: 0;
  }

  .sidebar-section-header .chevron.rotated {
    transform: rotate(90deg);
  }

  .sidebar-section-header > span:not(.chevron):first-of-type,
  .sidebar-section-header > span:nth-of-type(2) {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .section-count,
  .folder-count,
  .outline-page {
    margin-left: auto;
    color: var(--subtle);
    font-variant-numeric: tabular-nums;
  }

  .section-count {
    padding: 1px 6px;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--surface-raised);
    font-size: 11px;
    line-height: 1.35;
  }

  .folder-header {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    min-width: 0;
    padding: 6px 8px;
    border: none;
    background: none;
    color: var(--text);
    font-size: 13px;
    cursor: pointer;
    border-radius: 7px;
    text-align: left;
  }

  .folder-header:hover,
  .folder-header.selected {
    background: var(--surface-muted);
  }

  .folder-header :global(svg.rotated) {
    transform: rotate(90deg);
  }

  .folder-count {
    font-size: 11px;
  }

  .folder-docs {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 2px 0 4px 11px;
    padding-left: 10px;
    border-left: 1px solid var(--border);
  }

  .add-to-folder-btn {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 6px 8px;
    border: 1px dashed var(--border);
    background: none;
    color: var(--muted);
    font-size: 12px;
    cursor: pointer;
    border-radius: 7px;
    margin-bottom: 3px;
  }

  .add-to-folder-btn:hover {
    border-color: var(--accent);
    color: var(--accent);
    background: var(--accent-soft);
  }

  .doc-item {
    display: flex;
    align-items: center;
    gap: 7px;
    width: 100%;
    min-width: 0;
    padding: 6px 8px;
    border: none;
    background: none;
    color: var(--text);
    font-size: 13px;
    cursor: pointer;
    border-radius: 7px;
    text-align: left;
    line-height: 1.25;
  }

  .doc-item:hover {
    background: var(--surface-muted);
  }

  .doc-item.active {
    background: var(--accent-soft);
    color: var(--accent);
    font-weight: 600;
    box-shadow: inset 2px 0 0 var(--accent);
  }

  .doc-item.nested {
    padding-left: 8px;
  }

  .doc-item span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
    min-width: 0;
  }

  .doc-item .delete-btn {
    opacity: 0;
    transition: opacity 100ms ease;
  }

  .doc-item:hover .delete-btn,
  .folder-header:hover .delete-btn {
    opacity: 1;
  }

  .icon-btn-sm {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    border: 1px solid transparent;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    border-radius: 6px;
    flex-shrink: 0;
  }

  .icon-btn-sm:hover {
    background: var(--surface-muted);
    color: var(--accent);
  }

  .icon-btn-sm.prominent {
    border-color: var(--border);
    background: var(--surface-raised);
    color: var(--accent);
  }

  .icon-btn-sm.prominent:hover {
    border-color: var(--accent);
    background: var(--accent-soft);
  }

  .delete-btn:hover {
    color: var(--danger-text) !important;
    background: var(--danger-bg) !important;
  }

  .new-folder-row {
    display: flex;
    gap: 4px;
    padding: 2px 4px 6px;
  }

  .new-folder-row input {
    flex: 1;
    min-width: 0;
    height: 28px;
    padding: 0 8px;
    border: 1px solid var(--border);
    border-radius: 7px;
    font-size: 13px;
    background: var(--surface-raised);
    color: var(--text);
  }

  .recent-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .outline-tree {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .outline-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    min-width: 0;
    padding: 5px 8px;
    border: none;
    background: none;
    color: var(--text);
    font-size: 12px;
    cursor: pointer;
    border-radius: 6px;
    text-align: left;
    line-height: 1.35;
  }

  .outline-item:hover {
    background: var(--surface-muted);
  }

  .outline-page {
    font-size: 11px;
    flex-shrink: 0;
    margin-left: 8px;
  }

  .outline-item span:first-child {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .sidebar-empty {
    padding: 8px 10px;
    color: var(--subtle);
    font-size: 12px;
    line-height: 1.35;
  }
</style>
