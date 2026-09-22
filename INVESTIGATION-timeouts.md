# Investigation: timeout issues after moving from laptop to this machine

Date: 2026-09-21. Author: pi agent. Status: **could not reproduce a timeout on this machine** — full app flow through the live MPC editor works. Findings below.

## What the app is

Tauri 2 desktop app (Rust workspace) that drives headful Chrome over CDP (chromiumoxide 0.9.1)
through the MakePlayingCards editor, plus reqwest downloads of card images from Google Drive.

## This machine's environment (the "new" one)

- NixOS 25.11, kernel 6.12, Hyprland/Wayland session (`XDG_SESSION_TYPE=wayland`, `DISPLAY=:0`, `WAYLAND_DISPLAY=wayland-1`)
- `google-chrome-stable` and `chromium` both at **149.0.7827.200**
- `cargo-tauri` NOT in system PATH (only inside `nix develop` per flake.nix)
- `CHROME` env var NOT set in a plain shell (set to nixpkgs chromium by flake's default devShell)
- No proxy env vars; DNS/HTTPS to makeplayingcards.com and drive.usercontent.google.com is fast (~0.3s)
- chromiumoxide auto-detection: `CHROME` env → PATH lookup (`google-chrome-stable`, `chromium`, ...). Both fallback paths work here even without the dev shell.

## Evidence the app ran here and hit trouble

- journalctl: `target/debug/mpc-autofill` ran 2026-09-21 10:51; Chromium scopes started 10:53:48, then 3 rapid launches 11:01:37 / 11:01:51 / 11:02:48 (consistent with retry-after-timeout behavior)
- No coredumps, no crash logs.

## Empirical results (probes I wrote and ran on THIS machine)

Probes: `crates/automation/examples/probe.rs` and `probe_flow.rs`.

1. `cargo test -p autofill-core -p autofill-automation` — all pass.
2. CDP health probe, NixOS Chromium 149, headless AND headful (Wayland): launch 0.2–0.3s, page load ~5s, `Runtime.evaluate` round-trips **< 4 ms**, `find_element` ~50 ms. No timeouts.
3. Full app-flow probe against live site (`Driver::launch` → `goto_start` → `define_project(S30, 18, no foil)` → `page_to_fronts`): **all OK in ~19 s total**. No timeout.
4. `page_to_backs` in the probe fails with a site-side JS `TypeError: Cannot read properties of undefined (reading 'Upload')` in `checkAllowDo` — expected probe artifact (empty fronts; real app uploads fronts first). Not a timeout and not machine-specific.

## The timeout surface (all the timeouts that exist)

chromiumoxide 0.9.1 defaults, NOT overridden by this app (`BrowserConfig::builder()` never sets them):
- **30 s per CDP command** (`REQUEST_TIMEOUT = 30_000 ms`) — surfaces as `CdpError::Timeout` = "Request timed out."
- 20 s browser launch timeout.

`crates/automation/src/driver.rs` constants:
- `SPINNER_TIMEOUT` 30 s (wait_for_idle; reloads page once, then reports reloaded)
- `DEFINE_TIMEOUT` 10 s (waiting for editor globals `doPersonalize`, `setMode`, `oRenderFeature`, slots, editor iframe)
- `PRODUCT_SETTLE_TIMEOUT` 30 s (configurator selects)
- `UPLOAD_TIMEOUT` 300 s per image
- `SIGN_IN_TIMEOUT` 300 s ("timed out waiting for you to sign in")
- `CONTEXT_RETRY_TIMEOUT` 2 s; `find_element` ~5 s (25 × 200 ms); `POLL` 100 ms

`crates/core/src/download.rs`:
- **reqwest client has NO timeout configured at all** (`build_client()` sets only a user-agent). A stalled Drive connection hangs the Download phase *forever* with no error. Fresh machine → no `cards/` cache next to the order file → everything downloads here; the laptop likely had warm caches.

## Hypotheses for the user's "timeout issues", ranked

1. **SIGN_IN_TIMEOUT (5 min)**: every launch gets a fresh temp Chrome profile (chromiumoxide), so the user must sign in to MPC every run. On Hyprland the CDP-launched Chromium window can open on another workspace/unfocused; `Page.bringToFront` doesn't guarantee WM focus. User doesn't sign in within 300 s → "timed out waiting for you to sign in". Fits "worked on laptop" (different WM focus behavior).
2. **UI freeze outside the dev shell**: running `target/debug/mpc-autofill` directly (as the journal shows) skips the flake's `WEBKIT_DISABLE_DMABUF_RENDERER=1`, GSettings schema path, and `GIO_EXTRA_MODULES` (webview TLS). The flake comments say DMABUF "misbehaves under many NixOS GPU setups" — a frozen webview looks exactly like a hang/timeout. Note the app WAS launched directly at 10:51 (gvfs activation from the binary path).
3. **Silent download hang** (no error, but appears stuck): no reqwest timeout + concurrency 8 against Drive; if Drive throttles fresh connections, the Download phase can sit forever.
4. Genuine strict-timeout issue: NOT supported by evidence — CDP latency here is sub-millisecond; 30 s CDP timeout and 10 s define timeout are not being approached by anything I measured.

## What would pin it down (ask the user / next steps)

- The exact error text. Stage prefixes identify the failing stage precisely: `auth/wait`, `auth/check`, `fronts/personalize`, `backs/set-mode`, `editor-slot/wait-ready`, "Request timed out." (= 30 s CDP), "timed out waiting for you to sign in" (= 5 min sign-in), etc.
- How the app was launched: `nix develop -c cargo tauri dev` vs `cargo run` vs the built binary directly.
- Whether the order's `cards/` folder was copied over from the laptop.
- Consider regardless: add reqwest connect/read timeouts to `build_client()` (e.g. 30 s connect, 120 s total) so a stalled Drive fetch errors instead of hanging forever; consider raising/sign-posting `SIGN_IN_TIMEOUT` behavior on WMs that don't focus new windows.

## RESOLUTION (2026-09-21, after user supplied the error text)

User's error: "timed out waiting for required editor element `#uploadId`" — from
`find_element` (driver.rs), the tightest timeout in the codebase: 25 × 200 ms ≈ **5 s, single-shot**
(the `upload_with_retries` loop does NOT cover it; a find_element failure propagates via `?`).

Live probes (`probe_flow`, this machine, Chromium + Chrome 149, quantities 18 and 324):
`#uploadId` is **injected asynchronously by the editor's scripts ~2 s after `page_to_fronts`
returns** (main document, not the iframe; confirmed by polling both). So editor-readiness and
upload-input existence are decoupled, and 5 s is a race. Amplifiers in the user's real
(authenticated) session vs. the anonymous probe: account image-library/saved-state scripts,
and a spinner-forced reload inside `page_to_fronts`'s final `wait_for_idle`, which restarts
the entire editor init — and the `#uploadId` injection timeline — right before the upload.

**Fix applied**: `find_element` now polls until a `FIND_ELEMENT_TIMEOUT = 30 s` deadline
(matching the editor's other lazy-init budgets) instead of a fixed 25 iterations. Verified:
41/41 automation tests pass; live `upload_and_insert` still succeeds end-to-end.

**Follow-up fixes applied (2026-09-21)**:
1. `build_client()` (download.rs) now sets `connect_timeout(30s)` + `timeout(120s)` — a
   stalled Drive connection becomes a per-file error instead of hanging the Download phase.
2. `ExitRequested` (main.rs) now waits at most `SHUTDOWN_GRACE = 3 s` for the actor, then
   exits regardless — a hung command can no longer make the window un-closable.
3. `Driver::launch` now reaps orphaned automation browsers first (Linux `/proc` scan for the
   fixed `--user-data-dir=$TMP/chromiumoxide-runner` + `--remote-debugging-port` flags,
   SIGTERM → 2s grace → SIGKILL). NOTE: substring matching is required, not argv matching —
   NixOS wrappers repack the entire command line into argv[0]. Verified end-to-end: 11
   orphaned Chrome processes from a SIGKILLed run were reaped and the next launch succeeded
   (previously failed with `SingletonLock: File exists`).

Subagent review corrections folded in above: chromiumoxide uses a FIXED profile dir
(`/tmp/chromiumoxide-runner`, not per-run temp) → stale-profile collision hazard and ~10-day
sign-in persistence; `ExitRequested` shutdown blocks until the actor finishes its current
command (a stalled download makes the window un-closable); reqwest has no timeouts.

## Note

`INVESTIGATION-timeouts.md` and the two probe examples are untracked working files; delete or keep as desired.
