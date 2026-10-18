// Tauri doesn't have a Node.js server to do proper SSR
// so we use adapter-static with a fallback to index.html to put the site in SPA mode
// See: https://svelte.dev/docs/kit/single-page-apps
// See: https://v2.tauri.app/start/frontend/sveltekit/ for more info
import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),
  // Styles go inside each component's JS: when the webview reused cached modules after a dev
  // server restart, separate CSS modules failed to load ("failed to load virtual css module")
  // and parts of the window were unstyled. The app is local, so this costs nothing in production.
  compilerOptions: { css: "injected" },
  kit: {
    adapter: adapter({
      fallback: "index.html",
    }),
  },
};

export default config;
