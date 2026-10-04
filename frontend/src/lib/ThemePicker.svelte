<script lang="ts">
  import Icon from "./Icon.svelte";
  import { readTheme, saveTheme } from "./theme";

  let theme = readTheme();
</script>

<label class="theme-picker" title="Appearance">
  <Icon
    name={theme === "dark" ? "moon" : theme === "light" ? "sun" : "leaf"}
    size={15}
  />
  <select
    aria-label="Appearance"
    bind:value={theme}
    on:change={() => saveTheme(theme)}
  >
    <option value="light">Light (Default)</option>
    <option value="dark">Dark</option>
    <option value="zero">Zero</option>
  </select>
</label>
