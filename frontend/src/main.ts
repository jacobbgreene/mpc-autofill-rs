import "./app.css";
import App from "./App.svelte";
import { applyTheme, readTheme } from "./lib/theme";

applyTheme(readTheme());

export default new App({ target: document.getElementById("app")! });
