<script lang="ts">
  import Icon from "./Icon.svelte";
  import {
    phaseIndex,
    placementCounts,
    autofillRecovery,
    type Card,
    type Face,
  } from "./order";
  export let cards: Card[];
  export let phase: string;
  export let busy: boolean;
  export let error: string;
  export let downloadCompleted: number;
  export let downloadFailures: number;

  const steps = [
    "Prepare images",
    "Set up project",
    "Upload fronts",
    "Upload backs",
    "Review",
  ];
  let activeFace: Face;
  $: step = phaseIndex(phase);
  $: counts = placementCounts(cards);
  $: activeFace = step === 2 ? "front" : "back";
  $: faceCounts = placementCounts(cards, activeFace);
  $: uploading = phase === "uploading-fronts" || phase === "uploading-backs";
  $: attention = counts.attention > 0 || downloadFailures > 0;
  $: title = error
    ? "Autofill stopped"
    : ({
        downloading: "Preparing your card images",
        "starting-browser": "Opening the automation browser",
        "awaiting-sign-in": "Your turn: sign in to MakePlayingCards",
        configuring: "Setting up your project",
        "uploading-fronts": "Adding card fronts",
        "paging-to-backs": "Getting ready for card backs",
        "uploading-backs": "Adding card backs",
        "paging-to-review": "Opening project review",
        done: attention
          ? "Ready for review · check missing images"
          : "Your project is ready for review",
      }[phase] ?? "Ready when you are");
  $: description = error
    ? autofillRecovery(phase, error)
    : phase === "awaiting-sign-in"
      ? "Sign in in the Chrome window. Autofill will continue automatically."
      : phase === "done"
        ? `${counts.inserted} images added · ${counts.skipped} skipped. Review your cards in Chrome before ordering.`
        : phase === "downloading"
          ? `${downloadCompleted} of ${cards.length} images processed${downloadFailures ? ` · ${downloadFailures} failed` : ""}`
          : uploading
            ? `${faceCounts.inserted} of ${faceCounts.total} ${activeFace} images added${faceCounts.skipped ? ` · ${faceCounts.skipped} skipped` : ""}`
            : busy
              ? "Autofill is working in Chrome. You can keep browsing your cards here."
              : "Start autofill to prepare your images and open MakePlayingCards in Chrome.";
</script>

<section
  class="progress-panel"
  class:idle={!phase && !busy && !error}
  class:attention={phase === "awaiting-sign-in" ||
    (phase === "done" && attention)}
  class:has-error={!!error}
  aria-label="Autofill progress"
>
  <ol class="step-track">
    {#each steps as label, index}
      <li
        class:active={step === index}
        class:complete={step > index || phase === "done"}
        aria-current={step === index ? "step" : undefined}
      >
        <span class="step-number"
          >{#if step > index || phase === "done"}<Icon
              name="check"
              size={12}
            />{:else}{index + 1}{/if}</span
        >
        <span>{label}</span>
      </li>
    {/each}
  </ol>
  <div class="progress-body">
    <span
      class="progress-symbol"
      class:is-working={busy && phase !== "awaiting-sign-in" && !error}
    >
      <Icon
        name={error || (phase === "done" && attention)
          ? "warning"
          : phase === "awaiting-sign-in"
            ? "browser"
            : phase === "done"
              ? "check"
              : busy
                ? "upload"
                : "cards"}
        size={20}
      />
    </span>
    <div
      class="progress-copy"
      role="status"
      aria-live="polite"
      aria-atomic="true"
    >
      <h2>{title}</h2>
      <p>{description}</p>
    </div>
    {#if counts.attention}<button class="text-button attention-link" on:click
        >{counts.attention} need attention <Icon
          name="arrow"
          size={14}
        /></button
      >{/if}
  </div>
  {#if busy && !error && phase !== "awaiting-sign-in"}
    {#if phase === "downloading" && cards.length}
      <progress
        value={downloadCompleted}
        max={cards.length}
        aria-label="Image preparation"
      ></progress>
    {:else if uploading && faceCounts.total}
      <progress
        value={faceCounts.inserted + faceCounts.skipped}
        max={faceCounts.total}
        aria-label={`Processing ${activeFace} images`}
      ></progress>
    {:else}<progress aria-label="Setting up MakePlayingCards"></progress>{/if}
  {/if}
</section>
