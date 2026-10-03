<script lang="ts">
  import { onMount } from "svelte";
  import Icon from "./Icon.svelte";
  import { applyTheme, readTheme, saveTheme } from "./theme";

  let theme = readTheme();
  onMount(() => {
    const media = matchMedia("(prefers-color-scheme: dark)");
    const update = () => {
      if (theme === "system") applyTheme(theme);
    };
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  });
</script>

<label class="theme-picker" title="Appearance">
  <Icon
    name={theme === "dark" ? "moon" : theme === "light" ? "sun" : "monitor"}
    size={15}
  />
  <select
    aria-label="Appearance"
    bind:value={theme}
    on:change={() => saveTheme(theme)}
  >
    <option value="system">System</option>
    <option value="light">Light</option>
    <option value="dark">Dark</option>
  </select>
</label>
