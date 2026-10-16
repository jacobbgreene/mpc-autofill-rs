<script lang="ts">
  import license from "../../../LICENSE?raw";
  import notices from "../../../NOTICE.md?raw";
  import Icon from "./Icon.svelte";

  let dialog: HTMLDialogElement;
</script>

<footer class="legal-footer">
  <button class="text-button" on:click={() => dialog.showModal()}>
    About &amp; licenses
  </button>
</footer>

<dialog class="editor-dialog legal-dialog" bind:this={dialog} aria-labelledby="legal-title">
  <header class="editor-header">
    <div>
      <p class="eyebrow">ABOUT THIS APPLICATION</p>
      <h2 id="legal-title">MPC Autofill (Rust)</h2>
    </div>
    <button class="icon-button" aria-label="Close about and licenses" on:click={() => dialog.close()}>
      <Icon name="close" />
    </button>
  </header>
  <div class="editor-body">
    <p>
      An unofficial Rust/Tauri adaptation of MPC Autofill by chilli-axe and
      contributors, maintained separately from the upstream project.
    </p>
    <p>Rust/Tauri modifications copyright © 2026 jacobbgreene and contributors.</p>
    <p>
      You may redistribute and modify this software under the GNU General Public
      License version 3 only (GPL-3.0-only). It comes without any warranty,
      including merchantability or fitness for a particular purpose.
    </p>
    <p>Source repository: <code>https://github.com/jacobbgreene/mpc-autofill-rs</code></p>
    <p>
      If you received an installer without repository access, obtain its matching
      source archive from the person who supplied it.
    </p>
    <details>
      <summary>Attribution and modification notices</summary>
      <pre>{notices}</pre>
    </details>
    <details>
      <summary>GNU General Public License v3 — full text</summary>
      <pre>{license}</pre>
    </details>
  </div>
  <footer class="editor-footer">
    <button class="button secondary" on:click={() => dialog.close()}>Close</button>
  </footer>
</dialog>

<style>
  .legal-footer {
    padding: 16px 36px 24px;
    text-align: center;
  }
  .legal-dialog {
    width: min(720px, calc(100vw - 40px));
  }
  .editor-body {
    line-height: 1.6;
  }
  .editor-body p + p {
    margin-top: 12px;
  }
  details {
    margin-top: 18px;
  }
  summary {
    cursor: pointer;
    font-weight: 600;
  }
  pre, code {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font-size: 12px;
  }
  pre {
    padding: 14px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--surface);
  }
</style>
