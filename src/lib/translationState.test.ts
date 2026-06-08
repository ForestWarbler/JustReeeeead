import { describe, expect, it } from "vitest";

import {
  appendTranslationDelta,
  beginTranslation,
  createTranslationState,
  failTranslation,
  finishTranslation,
} from "./translationState";

describe("translation state", () => {
  it("streams deltas for the active job only", () => {
    let state = beginTranslation(createTranslationState(), "job-a", "source");
    state = appendTranslationDelta(state, "job-b", "ignored");
    state = appendTranslationDelta(state, "job-a", "hello");
    state = appendTranslationDelta(state, "job-a", " world");

    expect(state.output).toBe("hello world");
    expect(state.status).toBe("streaming");
  });

  it("finishes and fails only matching jobs", () => {
    let state = beginTranslation(createTranslationState(), "job-a", "source");
    state = finishTranslation(state, "job-b");
    expect(state.status).toBe("loading");

    state = failTranslation(state, "job-a", "bad key");
    expect(state.status).toBe("error");
    expect(state.error).toBe("bad key");
  });
});
