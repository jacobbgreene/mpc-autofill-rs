<script lang="ts">
  export let name = "cards";
  export let size = 18;
  const paths: Record<string, string> = {
    cards:
      "M8 3h11a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2ZM3 17V5a4 4 0 0 1 4-4m6.5 6 3.5 5-3.5 5-3.5-5 3.5-5Z",
    folder:
      "M3 7V5a2 2 0 0 1 2-2h5l2 3h7a2 2 0 0 1 2 2v2M3 7h15a2 2 0 0 1 2 2l-2 10H3L1 9a2 2 0 0 1 2-2Z",
    import: "M12 3v12m-4-4 4 4 4-4M4 15v4a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-4",
    arrow: "M5 12h14m-5-5 5 5-5 5",
    "chevron-left": "m15 5-7 7 7 7",
    "chevron-right": "m9 5 7 7-7 7",
    grid: "M3 3h7v7H3ZM14 3h7v7h-7ZM3 14h7v7H3ZM14 14h7v7h-7Z",
    check: "m5 12 4 4L19 6",
    moon: "M20.5 13A8.5 8.5 0 0 1 11 3.5 8.5 8.5 0 1 0 20.5 13Z",
    leaf: "M20 3C12 3 4 5 4 12a7 7 0 0 0 7 7c7 0 9-8 9-16ZM3 21 15 9",
    sun: "M12 2v2m0 16v2M2 12h2m16 0h2M5 5l1.5 1.5m11 11L19 19M5 19l1.5-1.5m11-11L19 5M16 12a4 4 0 1 1-8 0 4 4 0 0 1 8 0Z",
    monitor: "M3 3h18v13H3ZM8 21h8m-4-5v5",
    search: "M21 21l-5-5M18 10a8 8 0 1 1-16 0 8 8 0 0 1 16 0Z",
    close: "m6 6 12 12M6 18 18 6",
    warning: "m12 3 10 18H2L12 3Zm0 6v5m0 3h.01",
    clock: "M12 8v5l3 2m6-3a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z",
    upload: "M12 16V3m-4 4 4-4 4 4M4 15v4a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-4",
    browser:
      "M3 8h18M6 5h.01M9 5h.01M4 2h16a2 2 0 0 1 2 2v16a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2Z",
    image:
      "m3 17 5-5 4 4 3-3 6 6M8 7h.01M4 3h16a1 1 0 0 1 1 1v16a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1Z",
  };
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="1.7"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  <path d={paths[name] ?? paths.cards} />
</svg>
