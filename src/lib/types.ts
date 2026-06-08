export type TranslationDock = "right" | "bottom";
export type AppTheme = "light" | "dark";

export interface PdfDocumentInfo {
  docId: string;
  path: string;
  fileHash: string;
  title: string;
  pageCount: number;
  pages: PdfPageInfo[];
  outline: PdfOutlineItem[];
}

export interface PdfOutlineItem {
  title: string;
  page: number;
  children: PdfOutlineItem[];
}

export interface PdfPageInfo {
  width: number;
  height: number;
  rotation: number;
}

export type RenderQuality = "preview" | "final";

export interface PageRenderRequest {
  docId: string;
  pageIndex: number;
  zoom: number;
  dpr: number;
  rotation: number;
  quality: RenderQuality;
}

export interface PageTextLayer {
  pageIndex: number;
  spans: TextSpan[];
}

export interface TextSpan {
  id: string;
  text: string;
  bbox: RectDto;
  quad: QuadDto;
  fontSize: number;
  blockId: number;
  lineId: number;
}

export interface RectDto {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface QuadDto {
  x1: number;
  y1: number;
  x2: number;
  y2: number;
  x3: number;
  y3: number;
  x4: number;
  y4: number;
}

export interface AppSettings {
  version: 1;
  reader: ReaderSettings;
  layout: LayoutSettings;
  llmProviders: LlmProviderSettings[];
  promptProfiles: PromptProfile[];
}

export interface ReaderSettings {
  zoom: number;
  rotation: number;
  prefetchRadius: number;
  pageGap: number;
}

export interface LayoutSettings {
  translationOpen: boolean;
  translationDock: TranslationDock;
  translationSize: number;
  theme: AppTheme;
}

export interface LlmProviderSettings {
  id: string;
  name: string;
  baseUrl: string;
  model: string;
  apiKeyConfigured: boolean;
}

export interface PromptProfile {
  id: string;
  sourceLanguage: string;
  targetLanguage: string;
  promptTemplate: string;
}

export interface AppSettingsPatch {
  reader?: ReaderSettings;
  layout?: LayoutSettings;
  llmProviders?: LlmProviderSettings[];
  promptProfiles?: PromptProfile[];
}

export interface TranslationRequest {
  selectionText: string;
  sourceLanguage: string;
  targetLanguage: string;
  promptProfileId?: string;
  providerId: string;
}

export interface TranslationJob {
  jobId: string;
}

export interface TranslationStarted {
  jobId: string;
}

export interface TranslationDelta {
  jobId: string;
  delta: string;
}

export interface TranslationFinished {
  jobId: string;
  cancelled: boolean;
}

export interface TranslationError {
  jobId: string;
  message: string;
}

export interface LibraryFolder {
  id: string;
  name: string;
}

export interface LibraryDocument {
  id: string;
  path: string;
  title: string;
  fileHash: string;
  folderId: string | null;
  addedAt: string;
}

export interface LibraryData {
  folders: LibraryFolder[];
  documents: LibraryDocument[];
}

export interface RecentFileEntry {
  path: string;
  title: string;
  openedAt: string;
}
