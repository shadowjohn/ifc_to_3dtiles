import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";

function argValue(name, fallback) {
  const index = process.argv.indexOf(name);
  if (index < 0 || index + 1 >= process.argv.length) return fallback;
  return process.argv[index + 1];
}

const url = argValue("--url", "http://127.0.0.1:8120/index.html?phase2a=1");
const outputDir = path.resolve(argValue("--output-dir", "out/inspect_tamkang/publish/screenshots"));
const screenshotPath = path.join(outputDir, "phase2a_preview.png");
const reportPath = path.join(outputDir, "phase2a_visual_report.json");
fs.mkdirSync(outputDir, { recursive: true });

function writeReport(method, statsText = "", extra = {}) {
  const fallbackStats = statsText || statsTextFromPublishReport();
  const report = {
    phase: "2A",
    method,
    url,
    viewport: { width: 1440, height: 900, deviceScaleFactor: 1 },
    screenshot: screenshotPath,
    statsText: fallbackStats,
    generatedAt: new Date().toISOString(),
    ...extra
  };
  fs.writeFileSync(reportPath, JSON.stringify(report, null, 2), "utf8");
}

function statsTextFromPublishReport() {
  const publishDir = path.dirname(outputDir);
  const reportPath = path.join(publishDir, "geometry_preview", "geometry_publish_report.json");
  if (!fs.existsSync(reportPath)) return "";
  try {
    const report = JSON.parse(fs.readFileSync(reportPath, "utf8"));
    const categories = report.visual_category_counts || {};
    const categoryText = Object.keys(categories)
      .sort()
      .map(key => `${key}:${categories[key]}`)
      .join("  ") || "-";
    return [
      `triangles: ${report.triangle_count ?? 0}`,
      `surfaces: ${report.surface_feature_count ?? 0}`,
      `lines: ${report.line_feature_count ?? 0}`,
      `skipped: ${report.skipped_tiny_feature_count ?? 0}`,
      `debug markers: ${report.debug_marker_count ?? 0}`,
      `debug inflated: ${report.debug_inflated_feature_count ?? 0}`,
      `visual_category_counts: ${categoryText}`
    ].join("\n");
  } catch {
    return "";
  }
}

function browserCandidates() {
  return [
    "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
    "C:/Program Files/Microsoft/Edge/Application/msedge.exe",
    "C:/Program Files/Google/Chrome/Application/chrome.exe",
    "C:/Program Files (x86)/Google/Chrome/Application/chrome.exe"
  ];
}

async function captureWithPlaywright() {
  const { chromium } = await import("playwright");
  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 });
    await page.goto(url, { waitUntil: "domcontentloaded", timeout: 60000 });
    await page.waitForSelector("#geometryPreviewToggle", { timeout: 30000 });
    await page.evaluate(async () => {
      const preview = document.querySelector("#geometryPreviewToggle");
      if (preview && !preview.checked) preview.click();
      const surface = document.querySelector("#previewSurfacesToggle");
      const lines = document.querySelector("#previewLinesToggle");
      const markers = document.querySelector("#previewMarkersToggle");
      if (surface && !surface.checked) surface.click();
      if (lines && !lines.checked) lines.click();
      if (markers && !markers.checked) markers.click();
      await new Promise(resolve => setTimeout(resolve, 1200));
    });
    await page.screenshot({ path: screenshotPath, fullPage: false });
    const statsText = await page.locator("#geometryPreviewStats").textContent().catch(() => "");
    writeReport("playwright", statsText);
  } finally {
    await browser.close();
  }
}

function captureWithHeadlessBrowser(playwrightError) {
  const browserPath = browserCandidates().find(candidate => fs.existsSync(candidate));
  if (!browserPath) {
    throw new Error(`No Playwright package and no Edge/Chrome headless browser found. Playwright error: ${playwrightError.message}`);
  }
  const result = spawnSync(browserPath, [
    "--headless=new",
    "--disable-gpu",
    "--hide-scrollbars",
    "--window-size=1440,900",
    `--screenshot=${screenshotPath}`,
    url
  ], { encoding: "utf8" });
  if (result.status !== 0 || !fs.existsSync(screenshotPath)) {
    throw new Error(`Headless browser screenshot failed: ${result.stderr || result.stdout || result.status}`);
  }
  writeReport("edge_chrome_headless_fallback", "", {
    browserPath,
    playwrightError: playwrightError.message
  });
}

try {
  await captureWithPlaywright();
} catch (error) {
  captureWithHeadlessBrowser(error);
}

console.log(JSON.stringify({ screenshot: screenshotPath, report: reportPath }, null, 2));
