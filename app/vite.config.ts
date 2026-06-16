import { defineConfig } from "vite";
import { resolve } from "node:path";

// The verification battery lives in the sibling ../ts package (@satusd/sdk).
// We import its source directly (alias) and proxy the live LP + oracle so the
// browser never hits a cross-origin wall in dev — the PWA calls same-origin
// /lp and /oracle, Vite forwards them server-side.
export default defineConfig({
  resolve: {
    alias: {
      "@satusd/sdk/taproof": resolve(__dirname, "../ts/src/taproof.ts"),
      "@satusd/sdk/vault": resolve(__dirname, "../ts/src/vault.ts"),
      "@satusd/sdk/rail": resolve(__dirname, "../ts/src/rail.ts"),
      "@satusd/sdk": resolve(__dirname, "../ts/src/sdk.ts"),
    },
  },
  server: {
    // Bind all interfaces so a Tailscale device (phone/laptop) can reach it;
    // exposure stays private to the tailnet (dev box is behind NAT/firewall).
    host: true,
    allowedHosts: true,
    fs: { allow: [resolve(__dirname, ".."), __dirname] },
    proxy: {
      "/lp": {
        target: "http://207.148.98.132:9595",
        changeOrigin: true,
        rewrite: (p) => p.replace(/^\/lp/, ""),
      },
      "/oracle": {
        target: "http://207.148.98.132:9590",
        changeOrigin: true,
        rewrite: (p) => p.replace(/^\/oracle/, ""),
      },
      "/bridge": {
        target: "http://127.0.0.1:9596",
        changeOrigin: true,
        rewrite: (p) => p.replace(/^\/bridge/, ""),
      },
    },
  },
});
