# MPC Autofill (Rust)

A Rust/Tauri desktop app that auto-fills a card order into the MakePlayingCards
project editor. It imports a text decklist or an order XML, downloads the card images, and drives a
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
to an existing project, order combining/splitting, PDF export, and save-to-account.

## Working with an order

Select **Browse saved decks** on the welcome screen to open the deck library.
Search by name or path, sort by name or save date, and click a deck to open it. Use
**Saved decks** in the order toolbar to return; unsaved changes require confirmation.
**Back to start** returns to the original welcome screen.

New orders default to `Documents/MPC Autofill/Decks`, outside the source checkout
and installed application. **Save as…** can choose any other location. Orders
opened or saved elsewhere are remembered by path in `saved-decks.json` in the
platform's app data directory. On first use, existing valid order XML files
directly in Documents are also included without moving or copying them. The
library scans its default folder and refreshes remembered files when opened;
**Refresh** picks up external changes. Missing files are skipped with a notice.
If the platform has no Documents directory, the default deck folder is `decks`
inside the app data directory. Artwork thumbnails remain in the separate cache.

Start with **Import .txt decklist**, **Paste a decklist**, or drop a `.txt` file
onto the welcome screen. Importing a file opens an artwork grid immediately;
for pasted lists, select **Choose artwork**. Each new image starts with the first
MPC Autofill match. Cycle through options using the arrows beside its preview or
enter an artwork number. Click its thumbnail to inspect the current art in a
larger preview. **Browse all artwork** opens a separate, paginated
gallery with search and the current choice highlighted. The displayed image is
the one used in your deck. Unmatched cards can be searched by another name in
their gallery.

Previews are cached locally, with fallback between MPC Autofill's image CDN and
Google Drive. If a preview cannot load, use **Retry preview** on the card or
**Retry failed previews** in its gallery.

Expand **Print settings** to choose a name, cardstock and finish, and browse the
cardback gallery for a common reverse. **Edit decklist** returns to the text;
artwork choices for matching names are retained when you change quantities.
The suggested deck name is a placeholder that disappears as you type. Leave it
empty to use the imported filename (or **New deck** for a pasted list).
Explicit double-faced pairs keep their own backs. A common back is only required
for cards without an explicit reverse.

The appearance selector in the top bar offers **Light**, **Dark**, and **System**.
Your choice is remembered between sessions; **System** follows your device theme.

**Create deck** opens the completed draft in the card library. The first **Save**
asks where to write its XML order; the imported text file stays unchanged. The
**Open order** picker also accepts `.txt` files when another order is loaded.

Open or drop an order XML to browse its cards. Search by name, show all cards,
backs, or images needing attention, and click a card for a larger preview.
Use **Change artwork** below any image (or in its enlarged preview) to choose a
replacement from MPC Autofill. The common-back widget beside **Start autofill**
offers a larger preview and **Change back**, keeping the regular grid focused on
card fronts and individual backs. Common-back failures still appear under
**Needs attention**. A change applies to every copy using that image, preserving
quantities, slot order, and front/back pairings. Save the order to keep the new
artwork.
Autofill shows each stage and asks you to sign in in Chrome when necessary.
Completion reports skipped images so you can check them before placing an order.

**Edit list** opens the current order as text. **Replace deck** starts a new list
inside the same order. Both accept pasted text or an imported `.txt` file:

```text
4 Island
1 Lightning Bolt
1 Delver of Secrets | Insectile Aberration
```

Use one card per line with an optional quantity (`4` or `4x`). Blank lines and
lines beginning with `#` or `//` are ignored, along with category headings such
as `COMMANDER (1)` and `CREATURES (20)`. `Front name | Back name` specifies
a custom reverse. Existing artwork and back pairings are retained; new names
must have artwork chosen from the MPC Autofill search results before changes can
be applied. Use the full artwork name or image ID to distinguish variants with
the same card name. Each order supports up to 612 cards.

Applying a list updates the loaded order; **Save** writes it back as XML, and
**Save as…** writes a separate XML. Stock, foil, and the common cardback are
preserved. The app warns before discarding unsaved changes. Keyboard shortcuts:
`Ctrl/Cmd+F` searches cards, `Ctrl/Cmd+S` saves, and `Ctrl/Cmd+Shift+S` opens Save As.

Frontend validation (Node 22.6+):

```sh
npm --prefix frontend run check
npm --prefix frontend test
npm --prefix frontend run build
```

The normal Rust suite runs offline with `nix develop --command cargo test --workspace`.
An optional live artwork-search check is available with:

```sh
nix develop --command cargo test -p mpc-autofill -- --ignored
```
