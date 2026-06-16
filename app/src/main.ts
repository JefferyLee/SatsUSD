import "./styles.css";
import { renderHome } from "./views/home.ts";
import { renderBurn } from "./views/burn.ts";
import { renderTransfer } from "./views/transfer.ts";
import { renderBirth } from "./views/birth.ts";

type Stage = "home" | "birth" | "transfer" | "burn";

const STAGES: { id: Stage; label: string; sub: string }[] = [
  { id: "home", label: "Wallet", sub: "home" },
  { id: "birth", label: "Mint", sub: "birth" },
  { id: "transfer", label: "Verify", sub: "transfer" },
  { id: "burn", label: "Redeem", sub: "burn" },
];

const renderers: Record<Stage, (el: HTMLElement) => void> = {
  home: renderHome,
  birth: renderBirth,
  transfer: renderTransfer,
  burn: renderBurn,
};

function currentStage(): Stage {
  const h = location.hash.replace("#", "") as Stage;
  return STAGES.some((s) => s.id === h) ? h : "home";
}

function mount() {
  const root = document.getElementById("app")!;
  const stage = currentStage();
  root.innerHTML = `
    <header class="hdr">
      <div class="brand">SatUSD</div>
      <div class="tag">phone <b>verifies</b>, node <b>signs</b></div>
    </header>
    <nav class="stages">
      ${STAGES.map(
        (s) => `<a href="#${s.id}" class="stage ${s.id === stage ? "on" : ""}">
          <span class="s-label">${s.label}</span><span class="s-sub">${s.sub}</span>
        </a>`,
      ).join("")}
    </nav>
    <main id="view" class="view"></main>
    <footer class="ftr">signet · alpha · the phone trusts nothing it can't check</footer>
  `;
  renderers[stage](document.getElementById("view")!);
}

window.addEventListener("hashchange", mount);
mount();

if ("serviceWorker" in navigator) {
  window.addEventListener("load", () => {
    navigator.serviceWorker.register("/sw.js").catch(() => {});
  });
}
