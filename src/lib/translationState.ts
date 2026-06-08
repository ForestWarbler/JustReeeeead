export type TranslationStatus = "idle" | "loading" | "streaming" | "error";

export interface TranslationViewState {
  jobId: string | null;
  status: TranslationStatus;
  sourceText: string;
  output: string;
  error: string;
}

export function createTranslationState(): TranslationViewState {
  return {
    jobId: null,
    status: "idle",
    sourceText: "",
    output: "",
    error: "",
  };
}

export function beginTranslation(
  state: TranslationViewState,
  jobId: string,
  sourceText: string,
): TranslationViewState {
  return {
    ...state,
    jobId,
    sourceText,
    output: "",
    error: "",
    status: "loading",
  };
}

export function appendTranslationDelta(
  state: TranslationViewState,
  jobId: string,
  delta: string,
): TranslationViewState {
  if (state.jobId !== jobId) {
    return state;
  }
  return {
    ...state,
    output: state.output + delta,
    status: "streaming",
  };
}

export function finishTranslation(
  state: TranslationViewState,
  jobId: string,
): TranslationViewState {
  if (state.jobId !== jobId) {
    return state;
  }
  return {
    ...state,
    status: "idle",
  };
}

export function failTranslation(
  state: TranslationViewState,
  jobId: string,
  error: string,
): TranslationViewState {
  if (state.jobId !== jobId) {
    return state;
  }
  return {
    ...state,
    error,
    status: "error",
  };
}
