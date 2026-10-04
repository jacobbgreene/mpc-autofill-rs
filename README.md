# MPC Autofill (Rust)

A Rust/Tauri desktop app that auto-fills a card order into the MakePlayingCards
project editor. It imports a text decklist or an order XML, downloads the card images, and drives a
visible Chrome via the Chrome DevTools Protocol to build the project — all from a
single window.

## Installing a release

Download the package for your computer from
[GitHub Releases](https://github.com/jacobbgreene/mpc-autofill-rs/releases).
These initial preview builds target 64-bit Intel/AMD computers:

- **Windows 10/11:** run the `-setup.exe` installer. It installs WebView2 if
  necessary. The installer is currently unsigned.
- **Linux:** make the `.AppImage` executable and run it, or install the `.deb`
  with your distribution's package manager. The Linux packages are built on
  Ubuntu 22.04; other distributions still need compatibility testing.
- **NixOS:** use the native Nix package described below.

Install Google Chrome or Chromium separately for browser automation. You do not
need Rust, Node, or a development server to run the release packages. Autofill
uses its own browser profile, so your normal browser login is separate.
The repository is private: download links require repository access, or the
owner can send you the installer file directly.

## Choosing the browser

Set `CHROME` to the full path of your preferred browser before starting the app.
On this NixOS installation, build and launch with Google Chrome using:

```sh
nix build .#default
CHROME=/run/current-system/sw/bin/google-chrome-stable ./result/bin/mpc-autofill
```

The Nix package defaults to its bundled Chromium when `CHROME` is unset. Windows
and ordinary Linux packages detect an installed browser when it is unset.
Close the app before changing browsers. For development on NixOS, override the
variable **inside** the development environment:

```sh
nix develop --command env CHROME=/run/current-system/sw/bin/google-chrome-stable cargo-tauri dev
```

## Building release packages

The `Release` GitHub Actions workflow checks the frontend and Rust workspace,
builds a Windows NSIS installer plus Linux AppImage and Debian packages, and
checks that the packaged applications render and close through a native window
close request within five seconds. Windows is tested after a silent
installation; the Linux AppImage is extracted and started on a private display.
These checks do not exercise a signed-in MakePlayingCards upload.

Push a tag matching the versions in `Cargo.toml`, `src-tauri/tauri.conf.json`, and
`frontend/package.json` (for example, `v0.1.0`). Both platforms must pass before
the workflow creates a draft preview release with installers and SHA-256
checksums. Review the assets and publish the draft to share it. A manual workflow
run on a branch builds downloadable workflow artifacts without creating a release.

To reproduce packaging on a machine with
[Tauri's prerequisites](https://v2.tauri.app/start/prerequisites/) installed,
run these commands in Bash (Git Bash on Windows, so the npm launcher preserves
the `-- --locked` arguments):

```sh
npm --prefix frontend ci
npm install --global @tauri-apps/cli@2.11.4
tauri build --bundles nsis -- --locked          # Windows
tauri build --bundles appimage,deb -- --locked  # Linux
```

The icon uses the same card symbol as the application header. Regenerate its
platform variants with `cargo-tauri icon src-tauri/icons/icon.svg`.

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
onto the welcome screen. The text stays beside the artwork grid, and the preview
updates 350 ms after typing pauses. At enlarged display scaling the panes stack;
the action footer stays visible. The same workspace handles **Edit list** and
**Replace deck**. Each successful new search selects its first match. Quantity
changes, reordering, and unrelated corrections retain your artwork choices.
**Edit list** also shows the artwork already saved with your deck, without looking
it up again. At narrow widths, **Decklist** and **Artwork** jump buttons move
between the stacked panes. Correcting an issue does not move your cursor to another
issue or to the Apply button.

Correct names and quantities in the numbered text editor. Problems link to their
source lines; **Edit entry** or a tile's **Line** links focus and select that text.
Repeated names share an artwork choice and link to every contributing line.
After importing or pasting a complete list, the first unmatched entry is selected
once in the text editor, with a prominent **Card not found** alert and marked line
numbers. Typing or moving the caret cancels pending automatic navigation; further
edits and search results leave your cursor alone. Error links remain available,
and automatic navigation never takes focus from an open gallery.
**Needs attention** includes pending, unmatched, and failed searches plus a missing
common card back, text-validation problems, and preview failures. The **Card back** tile comes first in the artwork grid. Click its
card-shaped preview or **Choose card back** button to browse backs. It then shows
your chosen image, with **Change card back** available below. Its attention count
clears after selection.
Malformed
lines do not hide valid entries, but counts are marked incomplete and Create/Apply
stays disabled until the latest text and all required artwork are valid. Failed
searches offer **Retry**; names are never automatically corrected or guessed.

Cycle through artwork using the preview arrows or enter an artwork number. Click
the thumbnail for a larger preview. **Choose different artwork** (or **Find
artwork** for an unmatched entry) opens the gallery. **Search replacement artwork**
searches only for images when you press Enter or Search: choosing one does not
change the text entry's name. Use **Edit entry** in that gallery to return to its
source line. A new search clears old results, and a failed search identifies the
query with a retry action. Your previous selection stays selected until you choose
another image. Card backs have a separate **Search card backs** field; an empty
search browses all backs. Page controls and scrolling load additional results.

Previews are cached locally, with fallback between MPC Autofill's image CDN and
Google Drive. If a preview cannot load, use **Retry preview** on the card or
**Retry failed previews** in its gallery.

Use the always-visible **Print settings** section at the top to choose a name,
cardstock and finish.
These settings are also available through **Edit list** or **Edit print settings**
after creating a deck. Applying changes keeps them unsaved until **Save**. Deck
names come from XML filenames, so a renamed deck opens **Save as…** with the new
name suggested; the original file remains until you choose to replace it.

The deck viewer shows both physical card and artwork-image counts. **All artwork**
and **Backs** include the common back. Save status and the current filename remain
visible under the deck name. **More** contains **Replace deck**, **Save as…**, and
**Open order**. Missing thumbnails show **Retry preview** without blocking autofill.

To stop autofill after Chrome opens, close that automation browser. The loaded
deck remains available for editing or another run. An unfinished browser project
may not have been saved by MakePlayingCards; **Start over** creates a new project.
Error messages identify the failed stage and offer a next step, with technical
details below. Closing Chrome does not cancel the earlier image-download stage.

The final visual accessibility adjustments live in `frontend/src/accessibility.css`.
To undo just those appearance changes, remove its import from `frontend/src/main.ts`.
The workflow changes do not depend on that stylesheet.
The suggested deck name is a placeholder that disappears as you type. Leave it
empty to use the imported filename (or **New deck** for a pasted list).
Explicit double-faced pairs keep their own backs. A common back is only required
for cards without an explicit reverse.

The appearance selector in the top bar offers **Light (Default)**, **Dark**, and
**Zero**. Zero uses the charcoal and green dark palette from
[czrd/zero.nvim](https://github.com/czrd/zero.nvim), with its MIT notice included
in [the bundled theme license](frontend/public/zero-theme-LICENSE.txt).
Your choice is remembered between sessions and does not follow device appearance.
New installs and previously saved **System** preferences default to Light;
explicit Light and Dark preferences are preserved.

**Create deck** opens the completed draft in the card library. The first **Save**
asks where to write its XML order; the imported text file stays unchanged. The
**Open order** picker also accepts `.txt` files when another order is loaded.

Open or drop an order XML to browse its cards. Search by name, show all cards,
backs, or images needing attention, and click a card for a larger preview.
The grid, enlarged preview, and common-back widget show the original artwork's
DPI from MPC Autofill. Existing XML orders load this metadata in the background;
missing values show **DPI unavailable**, and failed lookups offer **Retry DPI**.
Use **Change artwork** below any image (or in its enlarged preview) to choose a
replacement from MPC Autofill. The common-back widget beside **Start autofill**
offers a larger preview and **Change back**, keeping the regular grid focused on
card fronts and individual backs. The card-back gallery loads more options as
you scroll, with a manual load-more button and retry if a request fails.
Common-back failures still appear under
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
preserved. Closing the title-bar window (or Alt+F4 on Windows) closes a saved,
idle deck immediately. Unsaved work offers **Discard and close** or **Keep
editing**; an untouched empty editor needs no confirmation. During autofill the
confirmation explains that closing stops the operation and closes its automation
browser. Repeated close requests share one confirmation. A failed close offers
Retry. Shutdown cancels network work, allows three seconds for graceful cleanup,
and bounds final cleanup so process exit completes within five seconds. Browser
ownership uses a dedicated persistent profile in the app's data directory,
protected by an OS lock. The previous generic browser profile is left untouched;
the first autofill after this upgrade requires signing in again. Startup and
shutdown never scan for or terminate other Chrome processes. Browser certificate
validation and Chrome's sandbox remain enabled.

Orders containing local file references show **Review local images**. Each image
requires a native **Allow image** confirmation, or you can choose a replacement
with the native file picker. Before approval, the app does not open, preview,
download, or upload the referenced file. Approved JPEG, PNG and WebP images are
copied into a private session cache; later changes to the original cannot change
what gets uploaded. References stay in the XML, and replacements still require
**Save**. Approval expires when another order is opened or the app exits.

Generated artwork is cached in the app cache, never in a `cards/` folder beside
an imported order. Cache filenames use source-specific hashes, and the webview
can access only preview files explicitly granted by the backend. Local artwork
must be an ordinary file: network shares, device paths, alternate data streams,
and symbolic links/junctions are rejected. Choose a regular copy if needed.

Import limits are enforced before constructing the deck: 612 cards, 8 MiB XML,
1 MB text lists, and bounded XML depth, records and fields. Artwork downloads are
limited to 64 MiB per original and 8 MiB per thumbnail, with a 64 megapixel and
256 MiB decoder allocation limit. Processing is serialized across downloads to
bound aggregate memory. An invalid or interrupted image is never treated as a
completed cache entry; incomplete downloads block autofill and can be retried.

Keyboard shortcuts:
`Ctrl/Cmd+F` searches cards, `Ctrl/Cmd+S` saves, and `Ctrl/Cmd+Shift+S` opens Save As.

Frontend validation (Node 22.12+ or Node 24):

```sh
npm --prefix frontend run check
npm --prefix frontend test
npm --prefix frontend exec -- playwright install chromium
npm --prefix frontend run test:browser
npm --prefix frontend run build
cargo test --workspace --locked
```

Browser tests mock Tauri commands and cover text correction, stale responses,
artwork retention, gallery search, focus, close decisions, and light/dark layouts
at 1100×820, 720×560 and a smaller CSS viewport representing enlarged scaling.
On a machine with Chrome installed, `PLAYWRIGHT_CHANNEL=chrome` uses that browser
instead of Playwright's Chromium. The separate browser-lifetime integration test
uses isolated test profiles and only `about:blank`:

```sh
cargo test -p autofill-automation --locked -- --ignored
```

Before publishing the next patch, run both package smoke tests and exercise X /
Alt+F4 on Windows and native close on Linux from the welcome screen, a saved deck,
an unsaved import, a nested artwork gallery, and active autofill. Check both
confirmation choices, automation-browser exit, and survival of an ordinary
browser window. Include Linux restart after an interrupted autofill to check
profile-lock recovery. The mocked browser tests do not replace these native
acceptance checks or a signed-in upload test. Publishing is a separate step.

Pull-request CI runs frontend checks, browser tests, Rust tests and isolated
Chrome lifetime tests on Windows and Linux. The dependency gate audits all
frontend dependencies (including build tools) and denies Rust vulnerabilities
and unsoundness advisories. A separate Nix gate checks the npm dependency hash
and builds the Linux package; its version is derived from Cargo.

GTK3 currently requires the GLib 0.18 Rust binding. The repository vendors that
binding with the narrow upstream fix for RUSTSEC-2024-0429; it does not replace
system GLib. `node scripts/check-glib-backport.mjs` verifies every vendored file,
and Linux CI runs the iterator regression with optimization. Review and remove
this backport when Tauri's GTK dependencies support the corrected upstream
series. Unmaintained transitive dependency notices remain visible in audit logs;
they are not broad vulnerability exemptions. Windows signing remains a release
follow-up requiring a signing identity; no signing credentials are stored here.

The normal Rust suite runs offline with `nix develop --command cargo test --workspace --locked`.
An optional live artwork-search check is available with:

```sh
nix develop --command cargo test -p mpc-autofill -- --ignored
```
