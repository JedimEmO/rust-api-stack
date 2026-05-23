import { defineConfig } from "vite";

export default defineConfig({
  server: {
    proxy: {
      "/rpc": "http://localhost:3000",
    },
  },
});
