import puppeteer from "puppeteer-core";

const CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const URL = process.env.URL ?? "http://localhost:5174/";

const browser = await puppeteer.launch({
  executablePath: CHROME,
  headless: "new",
  args: ["--no-sandbox", "--disable-gpu", "--hide-scrollbars"],
});
const page = await browser.newPage();
await page.setViewport({ width: 390, height: 880, deviceScaleFactor: 2 });
await page.goto(URL, { waitUntil: "networkidle0", timeout: 30000 });

// initial state (oracle price loaded + form)
await page.waitForSelector("#oracle-card .big", { timeout: 15000 }).catch(() => {});
await page.screenshot({ path: "/tmp/satusd-1-burn.png", fullPage: true });
console.log("shot 1: burn initial");

// verified state
await page.click("#go");
await page.waitForSelector("#result .banner", { timeout: 25000 });
await new Promise((r) => setTimeout(r, 400));
await page.screenshot({ path: "/tmp/satusd-2-verified.png", fullPage: true });
console.log("shot 2: verified battery");

await browser.close();
