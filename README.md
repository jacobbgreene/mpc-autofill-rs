# MPC Autofill (Rust)

A Rust/Tauri desktop app that auto-fills a card order into the MakePlayingCards
project editor. It parses an order XML, downloads the card images, and drives a
visible Chrome via the Chrome DevTools Protocol to build the project — all from a
single window.

This is a from-scratch reimagining of the reference Python desktop tool, focused
on a smoother, one-window experience.

## Layout

- `crates/core` — order model + XML parsing, Google Drive download, image
  post-processing. No GUI/browser deps; fast to build and unit-tested.
- `crates/automation` — the CDP driver (`chromiumoxide`) that steps through the
  MakePlayingCards editor.
- `src-tauri` — the Tauri app. A single session actor owns the parsed order,
  downloaded images, and the browser; the UI talks to it over a channel.
- `frontend` — the Svelte + Vite UI.

## Developing (NixOS)

The flake provides a NixOS-patched Rust toolchain plus Tauri's native
dependencies and a Chromium for automation.

```sh
# Fast shell for the logic crates (no WebKitGTK):
nix develop .#core --command cargo test

# Full shell for the app (first time: install the frontend deps):
nix develop
npm --prefix frontend install
cargo-tauri dev
```

Note the **hyphen** in `cargo-tauri dev`. Use it instead of `cargo tauri dev`:
cargo resolves the `tauri` subcommand from `~/.cargo/bin`, which on this machine
holds a non-NixOS binary that won't run; the hyphenated form uses the flake's
Tauri CLI directly.

`nix develop` exports `CHROME` so the driver launches the flake's Chromium.

CLI-free fallback (if the Tauri CLI ever misbehaves): run the dev server and the
app in two shells — `npm --prefix frontend run dev`, then `cargo run -p mpc-autofill`.

## Scope of this version

- Target: MakePlayingCards, new project only.
- Google Drive images are fetched from the public download endpoint (no service
  account / API key).

Not yet ported from the reference tool: the PrinterStudio sites, append/continue
to an existing project, order combining/splitting, PDF export, save-to-account,
and the single-cardback "same image" upload optimisation.
