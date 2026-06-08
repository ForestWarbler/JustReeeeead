# JustReeeeead

A fast AI-assisted PDF reader for papers.

## Stack

- Frontend: SvelteKit, Svelte 5, TypeScript, Vite
- Desktop shell: Tauri v2
- Native core: Rust 1.85.0 baseline
- PDF engine: MuPDF through `mupdf-rs`
- Translation: OpenAI-compatible streaming chat completions

## Features

- Open local PDFs with a native file dialog.
- Render pages through MuPDF via the `jrpage://` Tauri custom protocol.
- Use continuous virtualized scrolling with render prefetching and an LRU page-image cache.
- Overlay a transparent text layer for mouse selection.
- Translate selected text in a resizable right or bottom panel.
- Configure base URL, model, API key, language pair, and prompt template.
- Store settings in the OS app config directory and API keys in the OS keychain.

## Development

```sh
npm install
npm run tauri dev
```

## Checks

```sh
npm run check
npm test
npm run build
cd src-tauri
cargo test
cargo clippy --all-targets -- -D warnings
```

## License

This project is licensed as `AGPL-3.0-or-later` to match the open-source MuPDF dependency model.
