# GLib 0.18.5 security backport

This is the unmodified crates.io `glib` 0.18.5 package except for the two-line
`VariantStrIter::impl_get` fix described below. Its original MIT `LICENSE`,
`COPYRIGHT`, manifests, and version are retained. The local provenance files are
`PATCH.md`, `UPSTREAM-INTEGRITY.json`, and `.gitattributes`.

Original package SHA-256:
`233daaf6e83ae6a12a52055f568f9d7cf4671dabb78ff9560ab6da230ce00ee5`.

Upstream source revision recorded in the package:
`42b9caf98e03ded086362d9653ca58fe94dc8658`.

Fix: [gtk-rs-core PR 1343](https://github.com/gtk-rs/gtk-rs-core/pull/1343),
commit [`b5a4071e439bef2b5eea76c3aa25e5ae84839e34`](https://github.com/gtk-rs/gtk-rs-core/commit/b5a4071e439bef2b5eea76c3aa25e5ae84839e34).
In `src/variant_iter.rs`, the C output pointer is declared mutable and passed as
`&mut p`. No public API, ABI, feature, version, or dependency changes are made.

This corrects [RUSTSEC-2024-0429](https://rustsec.org/advisories/RUSTSEC-2024-0429.html).
The published fixed GLib series starts at 0.20, whereas this app's GTK3 and
WebKitGTK bindings require 0.18. The narrow backport avoids migrating the Linux
GUI stack or modifying any system-installed GLib library.

Verification:

- `node scripts/check-glib-backport.mjs` checks every original package file,
  the exact patched file, Cargo's patch selection, and the lockfile source.
- `node scripts/audit-rust.mjs` first audits the actual lockfile, then a disposable
  copy identifying GLib as its original registry package. Cargo-audit normally
  skips local path packages. The second scan excludes only RUSTSEC-2024-0429,
  whose exact fix was just verified, and fails on any other vulnerability or
  unsoundness advisory. The build lockfile is never rewritten by the scanner.
- On Linux, run
  `cargo test --release -p mpc-autofill --test glib_variant_security --locked`.
  Optimization is necessary because the original undefined behavior can be
  masked by debug builds. The regression covers forward/reverse iteration,
  nth/nth_back, last, Unicode/empty strings, and exhausted iterators.

Review this backport on GLib/GTK/Tauri updates. Remove it when the Linux GUI stack
can use a compatible upstream release containing the fix. Do not relabel the
package as version 0.20 merely to silence version-based scanners.
