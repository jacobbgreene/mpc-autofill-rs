<script context="module" lang="ts">
  import { invoke, convertFileSrc } from "@tauri-apps/api/core";
  import { createArtworkPreview } from "./artwork";
  const preview = createArtworkPreview(async (driveId) =>
    convertFileSrc(await invoke<string>("artwork_thumbnail", { driveId })),
  );
</script>

<script lang="ts">
  import { createEventDispatcher, onMount, onDestroy } from "svelte";
  import Icon from "./Icon.svelte";

  export let driveId: string;
  export let label: string;
  export let retry = 0;
  const dispatch = createEventDispatcher<{ error: string; loaded: void }>();
  let container: HTMLSpanElement;
  let visible = false;
  let src = "";
  let error = "";
  let loading = false;
  let generation = 0;
  let requestedId = "";
  let observer: IntersectionObserver | undefined;
  $: if (visible) void load(driveId, retry);
  onMount(() => {
    observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) {
          visible = true;
          observer?.disconnect();
        }
      },
      { rootMargin: "160px" },
    );
    observer.observe(container);
  });
  onDestroy(() => {
    generation++;
    observer?.disconnect();
  });
  async function load(id: string, attempt: number) {
    if (requestedId === id && !error) return;
    requestedId = id;
    const token = ++generation;
    src = "";
    error = "";
    loading = true;
    try {
      const path = await preview(id, attempt > 0);
      if (token === generation) src = path;
    } catch (e) {
      if (token === generation) fail(String(e));
    }
  }
  function fail(reason: string) {
    loading = false;
    error = reason;
    dispatch("error", reason);
  }
</script>

<span
  class="artwork-preview-image"
  bind:this={container}
  aria-busy={loading}
  title={error || undefined}
>
  {#if src && !error}
    {#key src}<img
        {src}
        alt={label}
        on:load={() => {
          loading = false;
          dispatch("loaded");
        }}
        on:error={() =>
          fail(
            "The cached preview could not be displayed. Try loading it again.",
          )}
      />{/key}
  {:else}<span class="artwork-placeholder"
      ><Icon name="image" size={28} /><span
        >{error ? "Preview unavailable" : "Loading preview…"}</span
      ></span
    >{/if}
</span>
