# MPC Autofill (Rust): attribution and modification notice

This is an unofficial Rust/Tauri adaptation of MPC Autofill. It is maintained
separately from the upstream project and is not an official upstream release.

## Upstream project

- Project: MPC Autofill — https://github.com/chilli-axe/mpc-autofill
- Authors and copyright holders: chilli-axe and the MPC Autofill contributors.
- Contributor history: https://github.com/chilli-axe/mpc-autofill/graphs/contributors
- License: GNU General Public License, version 3 only (`GPL-3.0-only`).
- Upstream license: https://github.com/chilli-axe/mpc-autofill/blob/ceb7c3b2f39b8c8caebe396ec87f2bddba6c3743/LICENSE.md

The upstream source inspected for this attribution on 2026-10-16 was commit
`ceb7c3b2f39b8c8caebe396ec87f2bddba6c3743`. The initial Rust commit does not
record the upstream revision used for the original port; this reference records
the inspected source, not a claim about that original revision.

The Rust implementation adapts the upstream desktop tool's order semantics and
browser automation. The principal source relationships are:

| Rust implementation | Upstream reference under `desktop-tool/src/` |
| --- | --- |
| `crates/automation/src/driver.rs` and `js.rs` | `driver.py` (editor flow, JavaScript calls, upload and placement) |
| `crates/automation/src/site.rs` | `constants.py` (site and product configuration) |
| `crates/core/src/order.rs` | `order.py` and `constants.py` (XML orders, image grouping, backs and cardstock) |
| `crates/core/src/image_proc.rs` | `processing.py` and `constants.py` (DPI calculation and downscaling) |

These references credit the upstream implementation; they do not claim that
every current line was copied from those files. Upstream authors retain their
copyright in upstream material.

## Rust/Tauri modifications

Copyright (C) 2026 jacobbgreene and contributors, for the Rust/Tauri modifications.

- 2026-09-20: initial Rust adaptation, replacing the Python/Selenium desktop
  implementation with Rust, Chrome DevTools Protocol automation, and a
  Tauri/Svelte interface.
- 2026-09-20 through 2026-10-16: redesigned deck and artwork workflows, saved
  decks and unfinished imports, themes, preview caching, automation reliability
  changes, platform packaging, and documentation. The Git history records
  individual changes and their dates.
- 2026-10-16: added explicit upstream attribution and legal notices, replaced
  the previous AGPL metadata with `GPL-3.0-only` to match upstream, and added
  source archives and notices to the release workflow. Historical tags and
  already-built installers retain their original contents and metadata.

This program is free software: you can redistribute it and/or modify it under
the terms of the GNU General Public License version 3 as published by the Free
Software Foundation. A complete copy is provided in `LICENSE`.

This program is distributed without any warranty, including the implied
warranties of merchantability or fitness for a particular purpose. See the
license for details.

## Distribution and other material

Distribute the license and this notice with the application. Release source
archives contain the exact repository revision used for the build, including
lockfiles and build scripts. Provide recipients access to the matching source
when sharing installers, including when they cannot access the private
repository. See the README for build instructions.

Third-party dependencies retain their own licenses and notices. Card artwork,
card names, and other third-party content shown in the application or screenshots
are not relicensed by this software license. The MPC Autofill community provides
the artwork-search and preview services used by the app.
