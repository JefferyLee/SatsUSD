/* SatUSD pitch page — all behavior. Zero build, GSAP via CDN with offline fallback. */
"use strict";

const $ = (id) => document.getElementById(id);
let LANG = "zh";
const T = (zh, en) => (LANG === "zh" ? zh : en);
const FONT = '-apple-system,BlinkMacSystemFont,"PingFang SC","Helvetica Neue",sans-serif';
const F = (px, w) => `${w ? w + " " : ""}${px}px ${FONT}`;
const MONO = '"SF Mono",ui-monospace,Menlo,monospace';
const FM = (px, w) => `${w ? w + " " : ""}${px}px ${MONO}`;
const ORANGE = "#f7931a", GREEN = "#2dd4a0", RED = "#ff5d5d", BLUE = "#6aa9ff",
      DIM = "#9a9aa3", FAINT = "#5b5b66", FG = "#f2f2f4", BG2 = "#0f0f14", LINE = "#222229";

function mulberry(seed) {
  return function () {
    seed |= 0; seed = (seed + 0x6D2B79F5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
const clamp01 = (v) => Math.max(0, Math.min(1, v));
const ease = (v) => v * v * (3 - 2 * v);

/* ---------- registries ---------- */
const scrubs = [];   // {id, fn, p}
function addScrub(id, fn) { scrubs.push({ id, fn, p: 0 }); }
function drawScrub(s) { const c = $(s.id); if (c) s.fn(c.getContext("2d"), c.width, c.height, s.p); }
const anims = new Set(); // rAF fns(t,ctx,W,H) bound below
function loop(t) { anims.forEach((f) => f(t)); requestAnimationFrame(loop); }
requestAnimationFrame(loop);
function addAnim(id, fn) {
  const c = $(id); if (!c) return () => {};
  const ctx = c.getContext("2d");
  const bound = (t) => fn(ctx, c.width, c.height, t);
  return bound;
}

/* ---------- i18n ---------- */
window.setLang = function (lang) {
  LANG = lang;
  document.documentElement.lang = lang;
  document.querySelectorAll("[data-zh]").forEach((el) => { el.innerHTML = el.dataset[lang]; });
  $("btn-zh").classList.toggle("on", lang === "zh");
  $("btn-en").classList.toggle("on", lang === "en");
  scrubs.forEach(drawScrub);
  drawTranche(); drawKill();
  rugVerdict && rugVerdict();
};

/* ════ S1 · three rulers (ambient) ════ */
function drawRulers(ctx, W, H, t) {
  ctx.clearRect(0, 0, W, H);
  const sec = t / 1000;
  const lanes = [
    { x0: 40,  label: T("法币", "FIAT"),   mode: "stretch", color: RED },
    { x0: 510, label: "BTC",               mode: "shake",   color: DIM },
    { x0: 980, label: "SatUSD",            mode: "steady",  color: ORANGE },
  ];
  const RW = 380, y = H * 0.52;
  // the constant object being measured
  lanes.forEach((l) => {
    ctx.fillStyle = "#1a1a20";
    ctx.fillRect(l.x0, y - 58, RW, 26);
    ctx.strokeStyle = "#333"; ctx.strokeRect(l.x0, y - 58, RW, 26);
  });
  lanes.forEach((l) => {
    ctx.strokeStyle = l.color; ctx.lineWidth = 3;
    ctx.beginPath(); ctx.moveTo(l.x0, y + 10); ctx.lineTo(l.x0 + RW, y + 10); ctx.stroke();
    const n = 11;
    for (let i = 0; i < n; i++) {
      let fx = i / (n - 1);
      if (l.mode === "stretch") {
        const s = 1 + 0.35 * (0.5 + 0.5 * Math.sin(sec * 0.5)); // slow inflation breathing
        fx = Math.min(1.04, fx * s);
      } else if (l.mode === "shake") {
        fx += (Math.sin(sec * 9 + i * 2.1) + Math.sin(sec * 17 + i)) * 0.013;
      }
      const x = l.x0 + fx * RW;
      if (x > l.x0 + RW + 18) continue;
      ctx.lineWidth = i % 5 === 0 ? 3 : 1.5;
      ctx.beginPath(); ctx.moveTo(x, y + 10); ctx.lineTo(x, y + 10 + (i % 5 === 0 ? 26 : 16)); ctx.stroke();
    }
    ctx.fillStyle = l.color; ctx.font = F(26, 600); ctx.textAlign = "left";
    ctx.fillText(l.label, l.x0, y + 78);
    ctx.fillStyle = FAINT; ctx.font = F(20);
    const sub = l.mode === "stretch" ? T("刻度在被拉开", "gradations stretching")
            : l.mode === "shake" ? T("刻度在震颤", "gradations vibrating")
            : T("刻度即美元，锚在比特币上", "dollar gradations, on Bitcoin bedrock");
    ctx.fillText(sub, l.x0, y + 108);
    if (l.mode === "steady") { // bedrock
      ctx.fillStyle = "rgba(247,147,26,.12)";
      ctx.fillRect(l.x0, y + 16, RW, 8);
    }
  });
  ctx.fillStyle = FAINT; ctx.font = F(19); ctx.textAlign = "left";
  ctx.fillText(T("被测量的物体（不变）", "the measured object (constant)"), 40, y - 70);
}

/* ════ S2 · valve particles ════ */
const valveR = mulberry(11);
const vParts = Array.from({ length: 130 }, (_, i) => ({
  x: -50 - valveR() * 1400, y: 0, lane: i % 5, top: i % 2 === 0, sp: 2 + valveR() * 2, dead: 0,
}));
function drawValve(ctx, W, H, t) {
  ctx.clearRect(0, 0, W, H);
  const sec = t / 1000;
  const closed = Math.floor(sec / 5) % 2 === 1; // valve closes lane 2 every other 5 s
  const yTop = H * 0.27, yBot = H * 0.76, vx = W * 0.52;
  // labels
  ctx.font = F(22, 600); ctx.textAlign = "left";
  ctx.fillStyle = DIM; ctx.fillText(T("法币轨道：所有流穿过一道阀门", "Fiat rails: every flow passes one valve"), 40, 44);
  ctx.fillText(T("比特币轨道：没有安装阀门的位置", "Bitcoin rails: no place to install a valve"), 40, H * 0.58);
  // valve body
  ctx.fillStyle = closed ? "rgba(255,93,93,.18)" : "rgba(154,154,163,.12)";
  ctx.fillRect(vx - 26, yTop - 86, 52, 172);
  ctx.strokeStyle = closed ? RED : FAINT; ctx.lineWidth = 2;
  ctx.strokeRect(vx - 26, yTop - 86, 52, 172);
  ctx.fillStyle = closed ? RED : FAINT; ctx.font = F(18, 600); ctx.textAlign = "center";
  ctx.fillText(closed ? T("对第 3 束流：关闭", "lane 3: CLOSED") : T("阀门：开放（暂时）", "valve: open (for now)"), vx, yTop - 100);
  vParts.forEach((p) => {
    const laneY = p.top ? yTop + (p.lane - 2) * 26 : yBot + (p.lane - 2) * 22;
    if (p.top && closed && p.lane === 2 && p.x > vx - 30 && !p.dead) p.dead = 1;
    if (p.dead) { p.x -= p.sp * 1.6; if (p.x < -40) { p.dead = 0; p.x = -40; } }
    else { p.x += p.sp * (p.top ? 1 : 1.15); if (p.x > W + 40) p.x = -40; }
    ctx.fillStyle = p.dead ? RED : p.top ? DIM : ORANGE;
    ctx.globalAlpha = p.dead ? 0.9 : 0.75;
    ctx.beginPath(); ctx.arc(p.x, laneY + (p.top ? 0 : Math.sin(p.x * 0.012 + p.lane) * 14), 3.4, 0, 7); ctx.fill();
    ctx.globalAlpha = 1;
  });
}

/* ════ S3 · monetization curve (scrub) ════ */
const curveR = mulberry(5);
const curveNoise = Array.from({ length: 320 }, () => curveR() - 0.5);
addScrub("curve-canvas", (ctx, W, H, p) => {
  ctx.clearRect(0, 0, W, H);
  const n = Math.max(2, Math.floor(320 * ease(p)));
  // supply staircase → asymptote
  ctx.strokeStyle = "#3a3a42"; ctx.lineWidth = 2; ctx.beginPath();
  for (let i = 0; i < n; i++) {
    const x = (i / 319) * W;
    const halvings = Math.floor((i / 319) * 8);
    const supply = 1 - Math.pow(0.5, halvings + (i / 319) * 8 % 1 * 0); // stepwise
    const y = H * 0.30 - supply * H * 0.16;
    i ? ctx.lineTo(x, y) : ctx.moveTo(x, y);
  }
  ctx.stroke();
  ctx.setLineDash([2, 6]); ctx.strokeStyle = FAINT;
  ctx.beginPath(); ctx.moveTo(0, H * 0.14); ctx.lineTo(W * p, H * 0.14); ctx.stroke(); ctx.setLineDash([]);
  ctx.fillStyle = FAINT; ctx.font = FM(19); ctx.textAlign = "left";
  if (p > 0.1) ctx.fillText(T("21,000,000 —— 已写死（第一种坏法：已修复）", "21,000,000 — hard-coded (breakage one: fixed)"), 16, H * 0.14 - 10);
  // adoption S-curve + price w/ vol envelope
  const S = (f) => 1 / (1 + Math.exp(-(f - 0.55) * 9));
  ctx.beginPath();
  for (let i = 0; i < n; i++) {
    const f = i / 319, x = f * W;
    const env = (1 - S(f)) * 0.5 + 0.02;
    const mid = H * (0.88 - S(f) * 0.42);
    const yv = mid + curveNoise[i] * env * H * 0.42;
    i ? ctx.lineTo(x, yv) : ctx.moveTo(x, yv);
  }
  ctx.strokeStyle = ORANGE; ctx.lineWidth = 2.4; ctx.stroke();
  // envelope shading
  ctx.globalAlpha = 0.10; ctx.fillStyle = ORANGE; ctx.beginPath();
  for (let i = 0; i < n; i++) { const f = i / 319; const env = (1 - S(f)) * 0.5 + 0.02; ctx.lineTo(f * W, H * (0.88 - S(f) * 0.42) - env * H * 0.42); }
  for (let i = n - 1; i >= 0; i--) { const f = i / 319; const env = (1 - S(f)) * 0.5 + 0.02; ctx.lineTo(f * W, H * (0.88 - S(f) * 0.42) + env * H * 0.42); }
  ctx.fill(); ctx.globalAlpha = 1;
  // here marker
  if (p > 0.35) {
    const f = 0.22, x = f * W, y = H * (0.88 - S(f) * 0.42);
    ctx.fillStyle = ORANGE; ctx.beginPath(); ctx.arc(x, y, 9, 0, 7); ctx.fill();
    ctx.font = F(24, 600); ctx.fillStyle = FG; ctx.textAlign = "left";
    ctx.fillText(T("我们在这里：早期 · 陡峭 · 嘈杂", "We are here: early · steep · loud"), x + 20, y - 16);
  }
  if (p > 0.75) {
    ctx.font = F(21); ctx.fillStyle = DIM; ctx.textAlign = "right";
    ctx.fillText(T("采用 ↑，波动包络 ↓ —— 第二种坏法由时间修复", "adoption ↑, envelope ↓ — breakage two heals with time"), W - 20, H * 0.40);
    ctx.fillStyle = ORANGE;
    ctx.fillText(T("SatUSD 桥接的就是这段路 →", "SatUSD bridges exactly this road →"), W - 20, H * 0.40 + 34);
  }
});

/* ════ S4 · tranche lab ════ */
const trR = mulberry(9);
const trPath = Array.from({ length: 260 }, () => trR() - 0.48);
function drawTranche() {
  const c = $("tranche-canvas"); if (!c) return;
  const ctx = c.getContext("2d"), W = c.width, H = c.height;
  const crash = Number($("crash").value) / 100;
  ctx.clearRect(0, 0, W, H);
  const claimY = H * 0.62;
  // reserve value path (wiggly), scaled by crash at right end
  let v = 1.5; const pts = [];
  for (let i = 0; i < 260; i++) {
    v = Math.max(0.4, v + trPath[i] * 0.05);
    const f = i / 259;
    const crashHere = crash * ease(clamp01((f - 0.45) / 0.4));
    pts.push({ x: f * W, val: v * (1 - crashHere) });
  }
  const vy = (val) => claimY - (val - 1.0) * H * 0.42;
  // cushion fill between reserve and claim
  ctx.globalAlpha = 0.14; ctx.fillStyle = ORANGE; ctx.beginPath();
  pts.forEach((p, i) => (i ? ctx.lineTo(p.x, vy(Math.max(1, p.val))) : ctx.moveTo(p.x, vy(Math.max(1, p.val)))));
  ctx.lineTo(W, claimY); ctx.lineTo(0, claimY); ctx.fill(); ctx.globalAlpha = 1;
  // reserve line
  ctx.strokeStyle = ORANGE; ctx.lineWidth = 2.6; ctx.beginPath();
  pts.forEach((p, i) => (i ? ctx.lineTo(p.x, vy(p.val)) : ctx.moveTo(p.x, vy(p.val))));
  ctx.stroke();
  // under-water shading if broken
  const endVal = pts[259].val;
  // claim line
  const broken = endVal < 1.0;
  ctx.strokeStyle = broken ? RED : GREEN; ctx.lineWidth = 3;
  if (broken) ctx.setLineDash([8, 7]);
  ctx.beginPath(); ctx.moveTo(0, claimY); ctx.lineTo(W, claimY); ctx.stroke(); ctx.setLineDash([]);
  ctx.font = F(22, 600); ctx.textAlign = "left";
  ctx.fillStyle = broken ? RED : GREEN;
  ctx.fillText(broken ? T("NAV 地板：赎回价 = CR × 面值，人人同价（spec 04 §5）", "NAV floor: redemption = CR × face, same for everyone (spec 04 §5)")
                      : T("$1.00 优先索取权 —— 纹丝不动", "$1.00 senior claim — does not move"), 16, claimY + 34);
  ctx.fillStyle = ORANGE; ctx.fillText(T("BTC 储备价值（次级层吸收全部波动）", "BTC reserve value (junior side absorbs all volatility)"), 16, vy(pts[20].val) - 18);
  // gauges
  const cr = 150 * (1 - crash);
  $("crash-val").textContent = "−" + Math.round(crash * 100) + "%";
  $("cr-val").textContent = Math.round(cr) + "%";
  $("cr-val").className = "gv " + (cr >= 130 ? "good" : cr >= 100 ? "accent" : "bad");
  $("claim-val").textContent = cr >= 100 ? "$1.00" : "$" + (cr / 100).toFixed(2) + " NAV";
  $("cushion-val").textContent = Math.max(0, Math.round(cr - 100)) + "%";
}

/* ════ S5 · one UTXO tree (scrub) ════ */
function nodeBox(ctx, x, y, w, h, label, sub, color, alpha) {
  if (alpha <= 0) return;
  ctx.globalAlpha = alpha;
  ctx.fillStyle = BG2; ctx.strokeStyle = color; ctx.lineWidth = 2;
  ctx.beginPath(); ctx.roundRect(x - w / 2, y - h / 2, w, h, 14); ctx.fill(); ctx.stroke();
  ctx.fillStyle = FG; ctx.font = F(25, 600); ctx.textAlign = "center";
  ctx.fillText(label, x, y - (sub ? 6 : -8));
  if (sub) { ctx.fillStyle = DIM; ctx.font = F(19); ctx.fillText(sub, x, y + 24); }
  ctx.globalAlpha = 1;
}
function link(ctx, x1, y1, x2, y2, alpha, color) {
  if (alpha <= 0) return;
  ctx.globalAlpha = alpha; ctx.strokeStyle = color || FAINT; ctx.lineWidth = 2;
  ctx.beginPath(); ctx.moveTo(x1, y1); ctx.lineTo(x2, y2); ctx.stroke(); ctx.globalAlpha = 1;
}
addScrub("tree2-canvas", (ctx, W, H, p) => {
  ctx.clearRect(0, 0, W, H);
  const a1 = ease(clamp01(p * 3.4));
  const a2 = ease(clamp01((p - 0.22) * 3.4));
  const a3 = ease(clamp01((p - 0.46) * 3.4));
  const a4 = ease(clamp01((p - 0.68) * 3.2));
  const cx = W * 0.42;
  link(ctx, cx - 200, H * 0.78 - 44, cx, H * 0.52 + 40, a2);
  link(ctx, cx + 200, H * 0.78 - 44, cx, H * 0.52 + 40, a2);
  link(ctx, cx, H * 0.52 - 40, cx, H * 0.22 + 44, a3);
  nodeBox(ctx, cx - 200, H * 0.78, 360, 86, T("TA 资产承诺叶", "TA commitment leaf"), T("「这枚 UTXO 上站着 N 个 SatUSD」", "“N SatUSD stand on this UTXO”"), ORANGE, a1);
  nodeBox(ctx, cx + 200, H * 0.78, 360, 86, T("退款叶 · CSV 超时", "Refund leaf · CSV timeout"), T("单边逃生门，不需要任何人", "unilateral escape, needs nobody"), GREEN, a1);
  nodeBox(ctx, cx, H * 0.52, 320, 80, "branch(TA, refund)", T("BIP-341 默克尔根", "BIP-341 merkle root"), DIM, a2);
  nodeBox(ctx, cx, H * 0.22, 460, 90, T("输出键 Q —— 链上只看见这 32 字节", "Output key Q — the chain sees only these 32 bytes"), "Q = P + TapTweak(P ‖ root)·G", ORANGE, a3);
  if (a3 > 0.3) {
    ctx.globalAlpha = a3; ctx.fillStyle = DIM; ctx.font = F(20); ctx.textAlign = "center";
    ctx.fillText(T("内部键 P = DLC 资金键：密钥路径就是结算通道", "Internal key P = the DLC funding key: the key path IS the settlement channel"), cx, H * 0.95);
    ctx.globalAlpha = 1;
  }
  // two spend arrows
  if (a4 > 0) {
    const qx = cx + 235, qy = H * 0.22;
    ctx.globalAlpha = a4;
    ctx.strokeStyle = ORANGE; ctx.lineWidth = 3;
    ctx.beginPath(); ctx.moveTo(qx, qy - 12); ctx.lineTo(qx + 150 * a4, qy - 12 - 40 * a4); ctx.stroke();
    ctx.strokeStyle = GREEN;
    ctx.beginPath(); ctx.moveTo(qx, qy + 12); ctx.lineTo(qx + 150 * a4, qy + 12 + 70 * a4); ctx.stroke();
    ctx.textAlign = "left"; ctx.font = F(21, 600);
    ctx.fillStyle = ORANGE; ctx.fillText(T("花法 1 · 密钥路径：CET 结算（oracle 解锁，1 秒）", "Spend 1 · key path: CET settlement (oracle-unlocked, 1 s)"), qx + 160, qy - 56);
    ctx.fillStyle = GREEN; ctx.fillText(T("花法 2 · 脚本路径：CSV 超时退款（谁都不需要）", "Spend 2 · script path: CSV timeout refund (needs nobody)"), qx + 160, qy + 96);
    ctx.globalAlpha = 1;
  }
});

/* ════ S6 · anticipation point + 2^20 → 16 (scrub) ════ */
addScrub("antic-canvas", (ctx, W, H, p) => {
  ctx.clearRect(0, 0, W, H);
  const a1 = ease(clamp01(p * 2.6));               // equation
  const a2 = ease(clamp01((p - 0.34) * 2.2));      // collapse
  // equation
  ctx.globalAlpha = a1;
  ctx.font = FM(64, 700); ctx.fillStyle = FG; ctx.textAlign = "center";
  ctx.fillText("S  =  R  +  e·P", W / 2, H * 0.2);
  ctx.font = F(20); ctx.fillStyle = DIM;
  ctx.fillText(T("未来签名的公钥", "pubkey of a future signature"), W / 2 - 285, H * 0.2 + 44);
  ctx.fillText(T("公开 nonce 承诺", "public nonce commitment"), W / 2 - 60, H * 0.2 + 44);
  ctx.fillText(T("挑战 × oracle 公钥", "challenge × oracle pubkey"), W / 2 + 215, H * 0.2 + 44);
  if (a1 > 0.5) {
    ctx.fillStyle = ORANGE; ctx.font = F(22, 600);
    ctx.fillText(T("对应的私钥：要等 oracle 真的签字那一刻才存在", "its private key: exists only the moment the oracle actually signs"), W / 2, H * 0.2 + 86);
  }
  ctx.globalAlpha = 1;
  // collapse field → 16 locks
  if (a2 > 0) {
    const cols = 64, rows = 12, top = H * 0.42, fieldH = H * 0.26;
    ctx.globalAlpha = 0.5 * (1 - a2 * 0.85);
    ctx.fillStyle = FAINT;
    for (let r = 0; r < rows; r++) for (let c = 0; c < cols; c++) {
      const fx = (c + 0.5) / cols, fy = (r + 0.5) / rows;
      const tx = ((Math.floor(fx * 16) + 0.5) / 16) * W;          // fold target
      const x = (fx * W) * (1 - a2) + tx * a2;
      const y = (top + fy * fieldH) * (1 - a2) + (H * 0.8) * a2;
      ctx.fillRect(x - 1.5, y - 1.5, 3, 3);
    }
    ctx.globalAlpha = 1;
    ctx.font = F(21); ctx.fillStyle = DIM; ctx.textAlign = "left";
    ctx.fillText(T("2²⁰ = 1,048,576 个价格结果", "2²⁰ = 1,048,576 price outcomes"), 30, top - 14);
    // 16 locks
    const la = ease(clamp01((p - 0.6) * 3));
    if (la > 0) {
      for (let i = 0; i < 16; i++) {
        const x = ((i + 0.5) / 16) * W, y = H * 0.8;
        ctx.globalAlpha = la;
        ctx.strokeStyle = ORANGE; ctx.lineWidth = 2; ctx.fillStyle = BG2;
        ctx.beginPath(); ctx.roundRect(x - 30, y - 34, 60, 68, 10); ctx.fill(); ctx.stroke();
        ctx.fillStyle = ORANGE; ctx.beginPath(); ctx.arc(x, y - 8, 7, 0, 7); ctx.fill();
        ctx.fillRect(x - 2.5, y - 6, 5, 22);
        ctx.globalAlpha = 1;
      }
      ctx.globalAlpha = la; ctx.font = F(21, 600); ctx.fillStyle = ORANGE; ctx.textAlign = "left";
      ctx.fillText(T("按二进制前缀折叠 → 16 把锁，每把预签一笔加密的结算交易", "folded by binary prefix → 16 locks, each a pre-signed encrypted settlement"), 30, H * 0.95);
      ctx.globalAlpha = 1;
    }
  }
});

/* ════ S6b · bucket grid ════ */
const bucketsEl = $("buckets"); const WIN = 9;
if (bucketsEl) for (let i = 0; i < 16; i++) {
  const b = document.createElement("div");
  b.className = "bucket"; b.id = "bk" + i;
  b.innerHTML = `<div class="lock">🔒</div><div>#${i}</div><div>$${(i * 65536).toLocaleString("en-US")}+</div>`;
  bucketsEl.appendChild(b);
}
function fireBuckets() {
  const w = $("bk" + WIN); if (!w) return;
  w.classList.add("win"); w.querySelector(".lock").textContent = "🔓";
  if (window.gsap) gsap.to("#tick-banner", { opacity: 1, duration: 0.6 });
  else $("tick-banner").style.opacity = 1;
}
function resetBuckets() {
  const w = $("bk" + WIN); if (!w) return;
  w.classList.remove("win"); w.querySelector(".lock").textContent = "🔒";
  if (window.gsap) gsap.to("#tick-banner", { opacity: 0, duration: 0.3 });
}

/* ════ S7 · EOTS extraction (scrub) ════ */
addScrub("eots-canvas", (ctx, W, H, p) => {
  ctx.clearRect(0, 0, W, H);
  const slide = ease(clamp01(p * 2.2));            // cards slide together
  const cancel = ease(clamp01((p - 0.4) * 3));     // k cancels
  const solve = ease(clamp01((p - 0.65) * 3));     // d extracted
  const cardW = 480, cardH = 120;
  const x1 = W * 0.26 + (W * 0.5 - W * 0.26) * slide * 0.35;
  const x2 = W * 0.74 - (W * 0.74 - W * 0.5) * slide * 0.35;
  const y = H * 0.22;
  [[x1, "s₁ = k + e₁·d", T("对 $60,123 的签名", "signature on $60,123")],
   [x2, "s₂ = k + e₂·d", T("对同一秒的另一个价格", "another price for the same second")]].forEach(([x, eq, sub], i) => {
    ctx.fillStyle = BG2; ctx.strokeStyle = i ? RED : DIM; ctx.lineWidth = 2;
    ctx.beginPath(); ctx.roundRect(x - cardW / 2, y - cardH / 2, cardW, cardH, 14); ctx.fill(); ctx.stroke();
    ctx.font = FM(40, 700); ctx.fillStyle = FG; ctx.textAlign = "center";
    ctx.fillText(eq, x, y + 4);
    ctx.font = F(19); ctx.fillStyle = DIM; ctx.fillText(sub, x, y + 38);
    // strike-through k when cancelling
    if (cancel > 0) {
      ctx.strokeStyle = RED; ctx.lineWidth = 3; ctx.globalAlpha = cancel;
      const kx = x - (i ? 92 : 92);
      ctx.beginPath(); ctx.moveTo(kx - 22, y + 12); ctx.lineTo(kx + 22, y - 14); ctx.stroke();
      ctx.globalAlpha = 1;
    }
  });
  ctx.font = F(20); ctx.fillStyle = FAINT; ctx.textAlign = "center";
  ctx.fillText(T("同一个预承诺 nonce k —— 这是公开可见的事实", "the same predetermined nonce k — publicly visible"), W / 2, y + 92);
  if (cancel > 0) {
    ctx.globalAlpha = cancel;
    ctx.font = FM(36, 600); ctx.fillStyle = DIM;
    ctx.fillText("s₁ − s₂ = (e₁ − e₂)·d", W / 2, H * 0.55);
    ctx.globalAlpha = 1;
  }
  if (solve > 0) {
    ctx.globalAlpha = solve;
    ctx.fillStyle = "rgba(255,93,93,.12)"; ctx.strokeStyle = RED; ctx.lineWidth = 2.5;
    const bw = 560, bh = 96, bx = W / 2, by = H * 0.74;
    ctx.beginPath(); ctx.roundRect(bx - bw / 2, by - bh / 2, bw, bh, 14); ctx.fill(); ctx.stroke();
    ctx.font = FM(42, 700); ctx.fillStyle = RED; ctx.textAlign = "center";
    ctx.fillText("d = (s₁ − s₂) / (e₁ − e₂)", bx, by + 6);
    ctx.font = F(21, 600); ctx.fillStyle = FG;
    ctx.fillText(T("私钥。掉进任何一个旁观者手里。", "The secret key. In any bystander’s hands."), bx, by + 78);
    ctx.globalAlpha = 1;
  }
});

/* ════ S8 · rug game ════ */
let rugT = 0, rugMarks = [], rugActive = false, rugLast = 0;
const rugStake = 0.10;
const rugFees = (t) => 0.001 * (2 * t + 0.15 * t * t);
function drawRug(ctx, W, H, t) {
  if (rugActive) { if (rugLast) rugT = Math.min(60, rugT + (t - rugLast) / 1000 * 2.2); rugLast = t; }
  ctx.clearRect(0, 0, W, H);
  const X = (e) => 70 + (e / 60) * (W - 110);
  const maxV = (rugStake + rugFees(60));
  const Y = (v) => H - 52 - (v / maxV) * (H - 110);
  // axes
  ctx.strokeStyle = LINE; ctx.lineWidth = 1;
  ctx.beginPath(); ctx.moveTo(70, 20); ctx.lineTo(70, H - 52); ctx.lineTo(W - 30, H - 52); ctx.stroke();
  ctx.fillStyle = FAINT; ctx.font = F(18); ctx.textAlign = "center";
  ctx.fillText(T("epoch →（rail 的一生）", "epochs → (a rail’s life)"), W / 2, H - 16);
  // curves up to rugT
  const steps = Math.max(2, Math.floor(rugT * 4));
  const seg = (fn, color, dash) => {
    ctx.strokeStyle = color; ctx.lineWidth = 2.6; if (dash) ctx.setLineDash([7, 6]);
    ctx.beginPath();
    for (let i = 0; i <= steps; i++) { const ee = (i / steps) * rugT; const v = fn(ee); i ? ctx.lineTo(X(ee), Y(v)) : ctx.moveTo(X(ee), Y(v)); }
    ctx.stroke(); ctx.setLineDash([]);
  };
  seg((e) => rugStake + rugFees(e), DIM);
  seg((e) => (rugStake + rugFees(e)) / 2, ORANGE);
  ctx.font = F(19, 600); ctx.textAlign = "left";
  ctx.fillStyle = DIM; ctx.fillText(T("沉没成本（质押 + 已留存费）", "sunk cost (stake + retained fees)"), 84, Y(rugStake + rugFees(rugT)) - 12);
  ctx.fillStyle = ORANGE; ctx.fillText("capacity = ½ × sunk", 84, Y((rugStake + rugFees(rugT)) / 2) + 28);
  // rug marks
  rugMarks.forEach((m) => {
    ctx.strokeStyle = RED; ctx.lineWidth = 3;
    const x = X(m), yv = Y((rugStake + rugFees(m)) / 2);
    ctx.beginPath(); ctx.moveTo(x - 9, yv - 9); ctx.lineTo(x + 9, yv + 9); ctx.moveTo(x + 9, yv - 9); ctx.lineTo(x - 9, yv + 9); ctx.stroke();
  });
}
let rugVerdict = null;
function setupRug() {
  const btn = $("rug-btn"), rst = $("rug-reset"), out = $("rug-out");
  if (!btn) return;
  let last = null;
  rugVerdict = () => { if (last) render(last); };
  function render(m) {
    const loot = (rugStake + rugFees(m)) / 2, burn = rugStake + rugFees(m);
    out.innerHTML = T(
      `抢到 <b class="accent">${loot.toFixed(4)} BTC</b> · 烧掉履历 <b>${burn.toFixed(4)} BTC</b> · 净亏 <b class="bad">−${(burn - loot).toFixed(4)} BTC（−50%）</b><br>任何时刻按下，结果都一样：这不是运气，是不变量。`,
      `Looted <b class="accent">${loot.toFixed(4)} BTC</b> · burned résumé <b>${burn.toFixed(4)} BTC</b> · net <b class="bad">−${(burn - loot).toFixed(4)} BTC (−50%)</b><br>Press at any moment — same result. Not luck: an invariant.`);
  }
  btn.addEventListener("click", () => { rugMarks.push(rugT); last = rugT; render(rugT); });
  rst.addEventListener("click", () => { rugT = 0; rugMarks = []; last = null; out.innerHTML = ""; });
}
setupRug();

/* ════ S9 · trust market ════ */
const mkR = mulberry(23);
const mkRails = [
  { fx: 0.82, fy: 0.62, r: 34, q: 0.9, zh: "Rail-1 · 单签 oracle · 1s", en: "Rail-1 · single oracle · 1 s" },
  { fx: 0.58, fy: 0.34, r: 30, q: 0.8, zh: "Rail-0 · RFQ 原子换", en: "Rail-0 · RFQ atomic swap" },
  { fx: 0.24, fy: 0.22, r: 26, q: 0.5, zh: "optimistic · 慢 · 低信任", en: "optimistic · slow · low trust" },
  { fx: 0.55, fy: 0.10, r: 22, q: 0.35, dashed: true, zh: "internal_twap（未来）", en: "internal_twap (future)" },
  { fx: 0.78, fy: 0.88, r: 28, q: 0.6, doomed: true, zh: "草台 rail", en: "fly-by-night rail" },
];
const mkParts = Array.from({ length: 90 }, () => ({ x: -20 - mkR() * 600, y: 0, tgt: 0, sp: 2.2 + mkR() * 2.4, born: false }));
function drawMarket(ctx, W, H, t) {
  ctx.clearRect(0, 0, W, H);
  const sec = (t / 1000) % 14;
  const boom = sec > 8 && sec < 11 ? (sec - 8) / 3 : sec >= 11 ? 1 : 0;
  // axes
  ctx.fillStyle = FAINT; ctx.font = F(18); ctx.textAlign = "left";
  ctx.fillText(T("← 慢", "← slow"), 30, H - 24);
  ctx.textAlign = "right"; ctx.fillText(T("快（1 秒赎回）→", "fast (1-second redemption) →"), W - 30, H - 24);
  ctx.save(); ctx.translate(26, H / 2); ctx.rotate(-Math.PI / 2); ctx.textAlign = "center";
  ctx.fillText(T("信任假设 多 ↑ / 少 ↓", "trust assumed: more ↑ / less ↓"), 0, 0); ctx.restore();
  mkRails.forEach((rl, i) => {
    const x = rl.fx * W, y = rl.fy * (H - 80) + 20;
    let q = rl.q + Math.sin(t / 1700 + i * 2) * 0.06;
    if (rl.doomed) q = rl.q * (1 - boom);
    rl._x = x; rl._y = y; rl._q = Math.max(0.02, q);
    const ring = rl.r + q * 26;
    ctx.globalAlpha = rl.doomed && boom >= 1 ? 0.18 : 1;
    if (rl.dashed) ctx.setLineDash([6, 6]);
    ctx.strokeStyle = rl.doomed ? RED : ORANGE; ctx.lineWidth = 2;
    ctx.beginPath(); ctx.arc(x, y, ring, 0, 7); ctx.stroke(); ctx.setLineDash([]);
    ctx.fillStyle = BG2; ctx.beginPath(); ctx.arc(x, y, rl.r * 0.62, 0, 7); ctx.fill();
    ctx.strokeStyle = FAINT; ctx.stroke();
    ctx.fillStyle = rl.doomed ? RED : FG; ctx.font = F(18, 600); ctx.textAlign = "center";
    ctx.fillText(T(rl.zh, rl.en), x, y + ring + 26);
    ctx.globalAlpha = 1;
    // explosion contained by tranche ring
    if (rl.doomed && boom > 0 && boom < 1) {
      ctx.save(); ctx.beginPath(); ctx.arc(x, y, ring, 0, 7); ctx.clip();
      ctx.globalAlpha = 0.5 * (1 - boom);
      ctx.fillStyle = RED; ctx.beginPath(); ctx.arc(x, y, ring * boom * 1.4, 0, 7); ctx.fill();
      ctx.restore(); ctx.globalAlpha = 1;
      ctx.fillStyle = RED; ctx.font = F(18, 600);
      ctx.fillText(T("爆炸 —— 困在自己的 tranche 里", "blast — contained in its own tranche"), x, y - ring - 14);
    }
  });
  // particles
  mkParts.forEach((p) => {
    if (!p.born || p.done) {
      const weights = mkRails.map((r) => r._q);
      const sum = weights.reduce((a, b) => a + b, 0);
      let pick = mkR() * sum, idx = 0;
      for (; idx < weights.length - 1 && pick > weights[idx]; idx++) pick -= weights[idx];
      p.tgt = idx; p.x = -20; p.y = mkR() * H; p.born = true; p.done = false;
    }
    const rl = mkRails[p.tgt];
    const dx = rl._x - p.x, dy = rl._y - p.y, d = Math.hypot(dx, dy);
    if (d < rl.r) { p.done = true; return; }
    p.x += (dx / d) * p.sp; p.y += (dy / d) * p.sp;
    ctx.fillStyle = ORANGE; ctx.globalAlpha = 0.6;
    ctx.beginPath(); ctx.arc(p.x, p.y, 2.6, 0, 7); ctx.fill(); ctx.globalAlpha = 1;
  });
}

/* ════ S10 · two loops ════ */
function drawLoops(ctx, W, H, t) {
  ctx.clearRect(0, 0, W, H);
  const sec = (t / 1000) % 9;
  const cy = H * 0.44;
  // LEFT: UST collapse
  const lx = W * 0.26;
  const shrink = sec / 9;
  const coreR = 52 * (1 - shrink * 0.85);
  ctx.strokeStyle = RED; ctx.lineWidth = 2;
  ctx.fillStyle = "rgba(255,93,93,.10)";
  ctx.beginPath(); ctx.arc(lx, cy, coreR, 0, 7); ctx.fill(); ctx.stroke();
  ctx.fillStyle = RED; ctx.font = F(20, 700); ctx.textAlign = "center";
  ctx.fillText("UST", lx, cy + 7);
  for (let i = 0; i < 14; i++) {
    const ang = (t / 900 + i * 0.45) % 7;
    const r = (150 - shrink * 95) - i * 4;
    if (r < coreR + 6) continue;
    ctx.fillStyle = RED; ctx.globalAlpha = 0.55;
    ctx.beginPath(); ctx.arc(lx + Math.cos(ang) * r, cy + Math.sin(ang) * r * 0.8, 4, 0, 7); ctx.fill();
    ctx.globalAlpha = 1;
  }
  if (shrink > 0.86) { // crack
    ctx.strokeStyle = RED; ctx.lineWidth = 2.5;
    ctx.beginPath(); ctx.moveTo(lx - 14, cy - 12); ctx.lineTo(lx + 4, cy + 2); ctx.lineTo(lx - 4, cy + 14); ctx.stroke();
  }
  ctx.fillStyle = DIM; ctx.font = F(20, 600);
  ctx.fillText(T("抵押环：每一圈都在吃掉自己的地基", "collateral loop: each turn eats its own foundation"), lx, H * 0.88);
  ctx.fillStyle = RED; ctx.font = F(18);
  ctx.fillText(T("发散 → 漩涡", "divergent → whirlpool"), lx, H * 0.88 + 30);
  // RIGHT: SatUSD information loop
  const rx = W * 0.72;
  const thick = 3 + ((t / 1000) % 30) * 0.5; // signal ring thickens (capped visually)
  ctx.fillStyle = "rgba(247,147,26,.10)";
  ctx.strokeStyle = ORANGE; ctx.lineWidth = 2.5;
  ctx.beginPath(); ctx.roundRect(rx - 56, cy - 56, 112, 112, 10); ctx.fill(); ctx.stroke();
  ctx.fillStyle = ORANGE; ctx.font = F(22, 700);
  ctx.fillText("BTC", rx, cy - 2);
  ctx.fillStyle = DIM; ctx.font = F(15); ctx.fillText(T("外生抵押 · 从不进环", "exogenous · never enters the loop"), rx, cy + 24);
  ctx.strokeStyle = ORANGE; ctx.lineWidth = Math.min(11, thick); ctx.globalAlpha = 0.85;
  ctx.beginPath(); ctx.arc(rx, cy, 150, 0, 7); ctx.stroke(); ctx.globalAlpha = 1;
  for (let i = 0; i < 16; i++) {
    const ang = -t / 1200 + i * 0.39;
    ctx.fillStyle = FG;
    ctx.beginPath(); ctx.arc(rx + Math.cos(ang) * 150, cy + Math.sin(ang) * 150, 4.5, 0, 7); ctx.fill();
  }
  const lbl = (txt, ang, color) => {
    ctx.fillStyle = color; ctx.font = F(18, 600);
    ctx.fillText(txt, rx + Math.cos(ang) * 205, cy + Math.sin(ang) * 205);
  };
  lbl(T("结算", "settle"), -Math.PI / 2, FG);
  lbl(T("→ 价格信号", "→ price signal"), -Math.PI / 6, ORANGE);
  lbl(T("→ 更多交易量", "→ more volume"), Math.PI / 2, FG);
  ctx.fillStyle = DIM; ctx.font = F(20, 600);
  ctx.fillText(T("信息环：每转一圈，信号更厚", "information loop: each turn, a thicker signal"), rx, H * 0.88);
  ctx.fillStyle = GREEN; ctx.font = F(18);
  ctx.fillText(T("收敛 → 飞轮", "convergent → flywheel"), rx, H * 0.88 + 30);
  // inset whale-trim (bottom center-left strip)
  const ix = W * 0.435, iy = H * 0.07, iw = 180, ih = 110;
  ctx.strokeStyle = LINE; ctx.strokeRect(ix, iy, iw, ih);
  ctx.fillStyle = "rgba(247,147,26,.10)"; ctx.fillRect(ix, iy + 38, iw, 30);
  ctx.strokeStyle = ORANGE; ctx.lineWidth = 2;
  ctx.beginPath(); ctx.moveTo(ix, iy + 53); ctx.lineTo(ix + iw, iy + 53); ctx.stroke();
  const wsec = (t / 1000) % 6;
  if (wsec > 3) {
    const drop = clamp01((wsec - 3) / 1.2);
    const wy = iy + 8 + drop * 26; // falls toward band, bounces at band top
    ctx.fillStyle = RED; ctx.beginPath(); ctx.arc(ix + iw * 0.72, Math.min(wy, iy + 34), 6, 0, 7); ctx.fill();
    if (drop >= 1) { ctx.strokeStyle = RED; ctx.lineWidth = 2;
      ctx.beginPath(); ctx.moveTo(ix + iw * 0.72 - 7, iy + 27); ctx.lineTo(ix + iw * 0.72 + 7, iy + 41);
      ctx.moveTo(ix + iw * 0.72 + 7, iy + 27); ctx.lineTo(ix + iw * 0.72 - 7, iy + 41); ctx.stroke(); }
  }
  ctx.fillStyle = FAINT; ctx.font = F(14); ctx.fillText(T("±5% 修剪带：鲸鱼被弹掉", "±5% trim: whale bounced"), ix + iw / 2, iy + ih + 18);
}

/* ════ S11 · trust window (scrub) ════ */
addScrub("window-canvas", (ctx, W, H, p) => {
  ctx.clearRect(0, 0, W, H);
  ctx.font = FM(30, 700); ctx.fillStyle = FG; ctx.textAlign = "center";
  ctx.fillText("capacity = ½ × (retained_fees + stake)", W / 2, 48);
  ctx.font = F(18); ctx.fillStyle = FAINT;
  ctx.fillText(T("公式从不改变 —— 改变的只是「谁保证它被执行」", "the formula never changes — only who guarantees its execution"), W / 2, 80);
  const stages = [
    { icon: "👤", zh: "Stage 1 · 人", en: "Stage 1 · a human", sub: T("公开脚本 + 任何人复算", "open script + anyone recomputes"), wf: 1 },
    { icon: "✍️", zh: "Stage 2 · 仪式", en: "Stage 2 · a ceremony", sub: T("预签配额树：超额从未被签名", "pre-signed tranches: over-cap never signed"), wf: 0.06 },
    { icon: "🔒", zh: "Stage 3 · 数学", en: "Stage 3 · mathematics", sub: T("covenant / BitVM 写进花费条件", "covenant / BitVM in the spend conditions"), wf: 0 },
  ];
  const pw = W / 3;
  stages.forEach((s, i) => {
    const x0 = i * pw + 36, bw = pw - 72, y = H * 0.5;
    const active = ease(clamp01((p - i * 0.3) * 3.2));
    ctx.globalAlpha = 0.25 + 0.75 * active;
    ctx.font = F(40); ctx.textAlign = "center";
    ctx.fillText(s.icon, x0 + bw / 2, y - 64);
    ctx.font = F(22, 700); ctx.fillStyle = FG; ctx.fillText(T(s.zh, s.en), x0 + bw / 2, y - 18);
    // epoch bar
    ctx.strokeStyle = LINE; ctx.strokeRect(x0, y + 6, bw, 34);
    // amber trust window
    const wpx = bw * s.wf * (i === 0 ? 1 : 1);
    if (wpx > 0) {
      ctx.fillStyle = "rgba(247,147,26,.30)";
      ctx.fillRect(x0 + (i === 1 ? bw - wpx : 0), y + 6, Math.max(wpx, s.wf > 0 ? 6 : 0), 34);
    }
    ctx.fillStyle = s.wf === 0 ? GREEN : ORANGE; ctx.font = F(17, 600);
    const wl = s.wf === 1 ? T("信任窗口：一整个 epoch", "trust window: a whole epoch")
            : s.wf > 0 ? T("窗口：边界上的一瞬", "window: an instant at the boundary")
            : T("窗口：关闭", "window: closed");
    ctx.fillText(wl, x0 + bw / 2, y + 66);
    ctx.fillStyle = DIM; ctx.font = F(16); ctx.fillText(s.sub, x0 + bw / 2, y + 94);
    ctx.globalAlpha = 1;
    if (i < 2) {
      ctx.fillStyle = FAINT; ctx.font = F(30); ctx.fillText("→", (i + 1) * pw, y + 24);
    }
  });
});

/* ════ S12 · kill switch ════ */
let killed = false;
function drawKill() {
  const c = $("kill-canvas"); if (!c) return;
  const ctx = c.getContext("2d"), W = c.width, H = c.height;
  ctx.clearRect(0, 0, W, H);
  const infra = [
    T("oracle daemon", "oracle daemon"), T("universe/证明服务", "universe/proof server"),
    T("披露镜像", "disclosure mirror"), T("报价板", "quote board")];
  const hx = W * 0.46, hy = H * 0.5, bx = W * 0.72, ex = W * 0.93;
  // founder infra
  infra.forEach((name, i) => {
    const y = 60 + i * 90;
    const dead = killed;
    ctx.globalAlpha = dead ? 0.25 : 1;
    ctx.strokeStyle = dead ? RED : FAINT; ctx.fillStyle = BG2; ctx.lineWidth = 1.5;
    ctx.beginPath(); ctx.roundRect(60, y - 26, 250, 52, 10); ctx.fill(); ctx.stroke();
    ctx.fillStyle = dead ? RED : DIM; ctx.font = F(19); ctx.textAlign = "center";
    ctx.fillText(name + (dead ? " ✕" : ""), 185, y + 7);
    ctx.strokeStyle = FAINT; ctx.globalAlpha = dead ? 0.06 : 0.4;
    ctx.beginPath(); ctx.moveTo(310, y); ctx.lineTo(hx - 70, hy); ctx.stroke();
    ctx.globalAlpha = 1;
  });
  // holder → bitcoin → exit (always alive)
  const glow = killed ? 1 : 0.55;
  const nodeC = (x, y, r, label, color) => {
    ctx.fillStyle = BG2; ctx.strokeStyle = color; ctx.lineWidth = 2.5;
    ctx.beginPath(); ctx.arc(x, y, r, 0, 7); ctx.fill(); ctx.stroke();
    ctx.fillStyle = color; ctx.font = F(19, 600); ctx.textAlign = "center";
    ctx.fillText(label, x, y + r + 26);
  };
  ctx.strokeStyle = GREEN; ctx.lineWidth = 4; ctx.globalAlpha = glow;
  ctx.beginPath(); ctx.moveTo(hx + 46, hy); ctx.lineTo(bx - 52, hy); ctx.moveTo(bx + 52, hy); ctx.lineTo(ex - 40, hy); ctx.stroke();
  ctx.globalAlpha = 1;
  nodeC(hx, hy, 44, T("持有者", "Holder"), FG);
  nodeC(bx, hy, 50, T("比特币节点", "Bitcoin node"), GREEN);
  nodeC(ex, hy, 38, T("CSV 退出", "CSV exit"), GREEN);
  if (killed) {
    ctx.fillStyle = GREEN; ctx.font = F(23, 700); ctx.textAlign = "center";
    ctx.fillText(T("✓ 验证与退出路径存活 —— 只依赖比特币还在出块", "✓ verification & exit alive — needs only that Bitcoin keeps producing blocks"), W * 0.62, H * 0.13);
  } else {
    ctx.fillStyle = FAINT; ctx.font = F(19); ctx.textAlign = "center";
    ctx.fillText(T("创始人基础设施只提供便利，不提供保证", "founder infrastructure provides convenience, never guarantees"), W * 0.62, H * 0.13);
  }
}
const killT = $("kill-toggle");
if (killT) killT.addEventListener("click", () => { killed = !killed; killT.classList.toggle("on", killed); drawKill(); });

/* ════ S13 · prospect (scrub) ════ */
addScrub("prospect-canvas", (ctx, W, H, p) => {
  ctx.clearRect(0, 0, W, H);
  const n = Math.max(2, Math.floor(300 * ease(p)));
  const base = (f) => Math.pow(f, 1.8) * 0.5;
  // red valved stablecoins
  ctx.globalAlpha = 0.16; ctx.fillStyle = RED; ctx.beginPath(); ctx.moveTo(0, H - 50);
  for (let i = 0; i < n; i++) { const f = i / 299; ctx.lineTo(f * W, H - 50 - base(f) * (H - 110)); }
  ctx.lineTo((n - 1) / 299 * W, H - 50); ctx.fill(); ctx.globalAlpha = 1;
  ctx.strokeStyle = RED; ctx.lineWidth = 2; ctx.beginPath();
  for (let i = 0; i < n; i++) { const f = i / 299; const y = H - 50 - base(f) * (H - 110); i ? ctx.lineTo(f * W, y) : ctx.moveTo(0, y); }
  ctx.stroke();
  ctx.fillStyle = RED; ctx.font = F(20, 600); ctx.textAlign = "left";
  if (p > 0.2) ctx.fillText(T("现有稳定币：数千亿美元 —— 全部带阀门", "today’s stablecoins: hundreds of billions — all valved"), 24, H - 66 - base(Math.min(1, n / 299)) * (H - 110));
  // unserved bands
  const bands = [
    { k: 0.55, color: ORANGE, zh: "资本管制 / 被去银行化的人口", en: "the capital-controlled / debanked", at: 0.45 },
    { k: 0.75, color: ORANGE, zh: "比特币经济圈（拒绝阀门的需求）", en: "the Bitcoin economy (valve-refusing demand)", at: 0.6 },
    { k: 1.05, color: BLUE, zh: "AI 智能体 —— 天生没有银行账户，只能用可验证的钱", en: "AI agents — born without bank accounts; verifiable money is all they can use", at: 0.75, dashed: true },
  ];
  bands.forEach((b) => {
    const a = ease(clamp01((p - b.at) * 4)); if (a <= 0) return;
    ctx.globalAlpha = a; ctx.strokeStyle = b.color; ctx.lineWidth = 2.4;
    if (b.dashed) ctx.setLineDash([8, 7]);
    ctx.beginPath();
    for (let i = 0; i < 300; i++) {
      const f = i / 299;
      const y = H - 50 - (base(f) + Math.pow(Math.max(0, f - 0.35), 2.2) * b.k) * (H - 110);
      i ? ctx.lineTo(f * W, Math.max(16, y)) : ctx.moveTo(0, H - 50 - base(0) * (H - 110));
    }
    ctx.stroke(); ctx.setLineDash([]);
    ctx.font = F(18, 600); ctx.fillStyle = b.color; ctx.textAlign = "right";
    const yEnd = Math.max(30, H - 50 - (base(1) + Math.pow(0.65, 2.2) * b.k) * (H - 110));
    ctx.fillText(T(b.zh, b.en), W - 16, yEnd - 8);
    ctx.globalAlpha = 1;
  });
});

/* ════ wiring ════ */
const hasGsap = typeof gsap !== "undefined" && typeof ScrollTrigger !== "undefined";
function runCounters() {
  document.querySelectorAll(".counter .n").forEach((el) => {
    const target = Number(el.dataset.count);
    if (!hasGsap) { el.textContent = target.toLocaleString("en-US"); return; }
    gsap.fromTo(el, { textContent: 0 }, {
      textContent: target, duration: 1.6, ease: "power2.out", snap: { textContent: 1 },
      onUpdate() { el.textContent = Number(el.textContent).toLocaleString("en-US"); },
    });
  });
}

if (hasGsap) {
  gsap.registerPlugin(ScrollTrigger);
  document.querySelectorAll(".reveal").forEach((el) => {
    gsap.to(el, { opacity: 1, y: 0, duration: 0.9, ease: "power3.out",
      scrollTrigger: { trigger: el, start: "top 84%" } });
  });
  scrubs.forEach((s) => {
    const c = $(s.id); if (!c) return;
    ScrollTrigger.create({ trigger: c, start: "top 80%", end: "bottom 35%", scrub: 0.4,
      onUpdate(self) { s.p = self.progress; drawScrub(s); } });
  });
  // rAF scenes activate in view
  [["ruler-canvas", drawRulers], ["valve-canvas", drawValve], ["rug-canvas", drawRug],
   ["market-canvas", drawMarket], ["loops-canvas", drawLoops]].forEach(([id, fn]) => {
    const bound = addAnim(id, fn);
    ScrollTrigger.create({ trigger: $(id), start: "top 90%", end: "bottom 5%",
      onToggle(self) {
        if (self.isActive) { anims.add(bound); if (id === "rug-canvas") { rugActive = true; rugLast = 0; } }
        else { anims.delete(bound); if (id === "rug-canvas") rugActive = false; }
      } });
  });
  ScrollTrigger.create({ trigger: "#buckets", start: "top 60%", onEnter: fireBuckets, onLeaveBack: resetBuckets });
  ScrollTrigger.create({ trigger: ".counters", start: "top 78%", once: true, onEnter: runCounters });
} else {
  // offline fallback: show everything, draw all finals, run all anims
  document.querySelectorAll(".reveal").forEach((el) => { el.style.opacity = 1; el.style.transform = "none"; });
  scrubs.forEach((s) => { s.p = 1; drawScrub(s); });
  [["ruler-canvas", drawRulers], ["valve-canvas", drawValve], ["rug-canvas", drawRug],
   ["market-canvas", drawMarket], ["loops-canvas", drawLoops]].forEach(([id, fn]) => anims.add(addAnim(id, fn)));
  rugActive = true;
  fireBuckets(); runCounters();
}

/* act rail */
const zones = document.querySelectorAll("[data-actzone]");
const actEls = { a1: null, a2: null, a3: null, a4: null };
document.querySelectorAll("#rail .act").forEach((el) => (actEls[el.dataset.act] = el));
const io = new IntersectionObserver((entries) => {
  entries.forEach((e) => {
    if (e.isIntersecting) {
      const act = e.target.dataset.actzone;
      Object.entries(actEls).forEach(([k, el]) => el && el.classList.toggle("on", k === act));
    }
  });
}, { rootMargin: "-40% 0px -50% 0px" });
zones.forEach((z) => io.observe(z));

/* initial paints */
drawTranche(); drawKill();
$("crash").addEventListener("input", drawTranche);
scrubs.forEach(drawScrub);
