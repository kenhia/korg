import { sveltekit } from "@sveltejs/kit/vite";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [tailwindcss(), sveltekit()],
  server: {
    port: 5173,
    proxy: {
      "/api": {
        target: process.env.KORG_API ?? "http://localhost:8080",
        changeOrigin: true,
      },
    },
  },
});
