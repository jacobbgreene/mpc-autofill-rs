import "./app.css";
import "./accessibility.css";
import App from "./App.svelte";
import { mount } from "svelte";
import { applyTheme, readTheme } from "./lib/theme";

applyTheme(readTheme());

export default mount(App, { target: document.getElementById("app")! });
