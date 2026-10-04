# Changelog

## 0.1.2

Changes since 0.1.1:

- Local artwork now requires explicit native approval before it can be read,
  previewed, or uploaded. Approved images use a private session snapshot, so
  changing the original file cannot silently change the upload.
- Hardened order imports, image downloads, decoding, and cache access with size
  limits, source-specific cache names, and protections against linked files and
  path traversal. Failed or incomplete downloads remain retryable.
- Automation uses an exclusively locked browser profile. Closing its browser
  stops the operation and returns to editing; app shutdown bounds cleanup and
  leaves unrelated browser windows alone. Chrome's sandbox and certificate
  validation remain enabled.
- Local and remote artwork with matching identifiers keep separate previews and
  progress. Replacing or reapproving local images refreshes the displayed snapshot.
- Added Blue, Purple, Pink, Orange, Teal, Sand, and Midnight appearances. Renamed
  Zero to Green, preserving existing preferences through migration.
- Clearer artwork loading and failure messages, including how to retry or review
  a local image.
- Reduced repeated rendering work while scrolling loaded decks. Larger decks may
  take slightly longer to render initially.
- Updated compatible Tauri dependencies, added dependency audits and native
  package checks, and backported the upstream GLib iterator security fix.

Upgrade notes:

- The new dedicated automation browser profile requires signing in again on the
  first autofill after upgrading. The previous profile is left untouched.
- Local artwork approval expires when another order opens or the app exits.
  Reopen **Review local images** to approve it again.
- Packages target Windows and Linux on x64. Windows installers remain unsigned.
  Signed-in MakePlayingCards uploads require separate manual acceptance testing.
