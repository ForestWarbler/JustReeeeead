# JustReeeeead v0.0.1

Initial MVP release of JustReeeeead, a fast desktop PDF reader for papers with AI-assisted translation.

## Highlights

- Open and read local PDF documents in a Tauri desktop app.
- Render PDF pages through the Rust native core and MuPDF.
- Use continuous scrolling with virtualized pages, preview/final progressive rendering, and render caching.
- Select PDF text through an overlay text layer.
- Translate selected text with OpenAI-compatible chat completion providers.
- Configure base URL, model, API key, language pair, and prompt template.
- Store API keys in the OS keychain instead of returning them to the frontend.
- Manage recent documents, library entries, and document chapters from the sidebar.
- Use light and dark UI themes with resizable translation/sidebar layouts.

## Notes

- This is an early MVP intended for paper reading and translation workflows.
- OCR, annotations, PDF editing, multi-document tabs, and account sync are not included yet.
- The project is licensed as AGPL-3.0-or-later because it uses the open-source MuPDF dependency model.
