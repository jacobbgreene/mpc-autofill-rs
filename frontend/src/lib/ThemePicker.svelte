<script lang="ts">
  import Icon from "./Icon.svelte";
  import { readTheme, saveTheme, themes } from "./theme";

  let theme = readTheme();
</script>

<label class="theme-picker" title="Appearance">
  <Icon
    name={themes.find((option) => option.value === theme)?.icon ?? "sun"}
    size={15}
  />
  <select
    aria-label="Appearance"
    bind:value={theme}
    on:change={() => saveTheme(theme)}
  >
    {#each themes as option}
      <option value={option.value}>{option.label}</option>
    {/each}
  </select>
</label>
