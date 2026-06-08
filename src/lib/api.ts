import { invoke } from "@tauri-apps/api/core";
import type {
  AppSettings,
  AppSettingsPatch,
  LibraryData,
  PageRenderRequest,
  PageTextLayer,
  PdfDocumentInfo,
  RecentFileEntry,
  TranslationJob,
  TranslationRequest,
} from "$lib/types";

export function openPdf(path: string): Promise<PdfDocumentInfo> {
  return invoke("open_pdf", { path });
}

export function closePdf(docId: string): Promise<void> {
  return invoke("close_pdf", { docId });
}

export function getPageTextLayer(docId: string, pageIndex: number): Promise<PageTextLayer> {
  return invoke("get_page_text_layer", { docId, pageIndex });
}

export function getPageRenderUrl(request: PageRenderRequest): Promise<string> {
  return invoke("get_page_render_url", { request });
}

export function prefetchPageRenders(requests: PageRenderRequest[]): Promise<void> {
  return invoke("prefetch_page_renders", { requests });
}

export function getSettings(): Promise<AppSettings> {
  return invoke("get_settings");
}

export function saveSettings(settings: AppSettingsPatch): Promise<AppSettings> {
  return invoke("save_settings", { settings });
}

export function saveApiKey(providerId: string, apiKey: string): Promise<void> {
  return invoke("save_api_key", { providerId, apiKey });
}

export function translateSelection(payload: TranslationRequest): Promise<TranslationJob> {
  return invoke("translate_selection", { payload });
}

export function cancelTranslation(jobId: string): Promise<void> {
  return invoke("cancel_translation", { jobId });
}

export function getRecentFiles(): Promise<RecentFileEntry[]> {
  return invoke("get_recent_files");
}

export function getLibrary(): Promise<LibraryData> {
  return invoke("get_library");
}

export function addLibraryFolder(name: string): Promise<LibraryData> {
  return invoke("add_library_folder", { name });
}

export function removeLibraryFolder(folderId: string): Promise<LibraryData> {
  return invoke("remove_library_folder", { folderId });
}

export function addLibraryDocument(
  path: string,
  title: string,
  fileHash: string,
  folderId: string | null,
): Promise<LibraryData> {
  return invoke("add_library_document", { path, title, fileHash, folderId });
}

export function removeLibraryDocument(docId: string): Promise<LibraryData> {
  return invoke("remove_library_document", { docId });
}
