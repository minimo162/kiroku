// src/copilot_server.ts
import * as fs from "node:fs";
import * as http from "node:http";
import * as os from "node:os";
import * as path from "node:path";
import {
  chromium
} from "playwright";
var DEFAULT_PORT = 18080;
var DEFAULT_CDP_PORT = 9222;
var COPILOT_URL = "https://m365.cloud.microsoft/chat/";
var INPUT_SELECTOR = '#m365-chat-editor-target-element, [data-lexical-editor="true"]';
var NEW_CHAT_BUTTON_SELECTOR = '[data-testid="newChatButton"]';
var SEND_BUTTON_SELECTOR = '.fai-SendButton:not([disabled]), button[aria-label*="Send"]:not([disabled]), button[aria-label*="\u9001\u4FE1"]:not([disabled])';
var STOP_BUTTON_SELECTOR = ".fai-SendButton__stopBackground";
var PLUS_BUTTON_SELECTORS = [
  '[data-testid="PlusMenuButton"]',
  'button[aria-label*="Add"]',
  'button[aria-label*="Upload"]',
  'button[aria-label*="\u6DFB\u4ED8"]',
  'button[aria-label*="\u30A2\u30C3\u30D7\u30ED\u30FC\u30C9"]'
];
var FILE_INPUT_SELECTORS = [
  '[data-testid="uploadFileDialogInput"]',
  'input[type="file"][accept*="image"]',
  'input[type="file"]'
];
var ATTACHMENT_READY_SELECTORS = [
  '[data-testid*="attachment"]',
  '[data-testid*="upload"]',
  '[data-testid*="image"]',
  '[aria-label*="Remove attachment"]',
  '[aria-label*="\u6DFB\u4ED8\u3092\u524A\u9664"]'
];
var RESPONSE_SELECTORS = [
  '[data-testid="markdown-reply"]',
  'div[data-message-type="Chat"]',
  'article[data-message-author-role="assistant"]'
];
var RESPONSE_URL_PATTERN = /substrate\.office\.com|copilot\.microsoft\.com|m365\.cloud\.microsoft|api\.bing\.microsoft\.com/i;
var RESPONSE_TIMEOUT_MS = 12e4;
var CopilotSession = class {
  browser = null;
  page = null;
  lock = false;
  async connect(cdpPort) {
    if (!this.browser || !this.browser.isConnected()) {
      this.browser = await chromium.connectOverCDP(`http://127.0.0.1:${cdpPort}`);
      this.browser.on("disconnected", () => {
        this.browser = null;
        this.page = null;
      });
    }
    this.page = await this.findOrCreateCopilotPage(this.browser);
  }
  async describe(systemPrompt, userPrompt, imageB64) {
    if (this.lock) {
      throw new Error("Copilot session is busy");
    }
    this.lock = true;
    let uploadedImagePath = null;
    try {
      await this.connect(globalOptions.cdpPort);
      const page = this.page;
      if (!page) {
        throw new Error("Copilot page is not available");
      }
      await page.bringToFront();
      if (!page.url().includes("m365.cloud.microsoft/chat")) {
        await page.goto(COPILOT_URL, { waitUntil: "domcontentloaded" });
      }
      await this.startNewChat(page);
      if (imageB64) {
        uploadedImagePath = await uploadImage(page, imageB64);
      }
      await pastePrompt(page, systemPrompt, userPrompt);
      return await submitPrompt(page);
    } finally {
      if (uploadedImagePath) {
        const imagePathForCleanup = uploadedImagePath;
        setTimeout(() => {
          fs.promises.unlink(imagePathForCleanup).catch(() => {
          });
        }, 5e3);
      }
      this.lock = false;
    }
  }
  async close() {
    if (this.browser) {
      await this.browser.close().catch(() => {
      });
    }
    this.browser = null;
    this.page = null;
    this.lock = false;
  }
  async findOrCreateCopilotPage(browser) {
    for (const context2 of browser.contexts()) {
      const existingPage = findCopilotPage(context2);
      if (existingPage) {
        return existingPage;
      }
    }
    const context = browser.contexts()[0];
    if (!context) {
      throw new Error("No existing Edge browser context found. Launch Edge with remote debugging enabled.");
    }
    const page = await context.newPage();
    await page.goto(COPILOT_URL, { waitUntil: "domcontentloaded" });
    return page;
  }
  async startNewChat(page) {
    const newChatButton = page.locator(NEW_CHAT_BUTTON_SELECTOR).first();
    if (await newChatButton.count() > 0) {
      await newChatButton.click({ timeout: 1e4 }).catch(() => {
      });
      await page.waitForTimeout(500);
    }
  }
};
function findCopilotPage(context) {
  for (const page of context.pages()) {
    if (page.url().includes("m365.cloud.microsoft/chat")) {
      return page;
    }
  }
  return null;
}
async function pastePrompt(page, systemPrompt, userPrompt) {
  const fullPrompt = systemPrompt ? `${systemPrompt}

${userPrompt}` : userPrompt;
  const inputEl = await page.waitForSelector(INPUT_SELECTOR, {
    state: "visible",
    timeout: 1e4
  });
  await inputEl.click();
  await page.keyboard.press(process.platform === "darwin" ? "Meta+A" : "Control+A").catch(() => {
  });
  await page.keyboard.press("Backspace").catch(() => {
  });
  await page.evaluate((text) => {
    const el = document.querySelector("#m365-chat-editor-target-element") ?? document.querySelector('[data-lexical-editor="true"]');
    if (!el) {
      return;
    }
    const dataTransfer = new DataTransfer();
    dataTransfer.setData("text/plain", text);
    el.dispatchEvent(new ClipboardEvent("paste", { clipboardData: dataTransfer, bubbles: true }));
  }, fullPrompt);
  await page.waitForTimeout(300);
  const currentText = await inputEl.innerText().catch(() => "");
  if (!currentText.trim()) {
    await inputEl.click();
    await page.keyboard.type(fullPrompt);
  }
}
async function submitPrompt(page) {
  const responsePromise = page.waitForResponse(
    (candidate) => RESPONSE_URL_PATTERN.test(candidate.url()) && candidate.status() === 200 && candidate.request().method() === "POST" && isLikelyCopilotCompletion(candidate),
    { timeout: RESPONSE_TIMEOUT_MS }
  ).catch(() => {
  });
  await page.locator(SEND_BUTTON_SELECTOR).first().click({ timeout: 1e4 });
  await responsePromise;
  return await waitForDomResponse(page);
}
function isLikelyCopilotCompletion(response) {
  const contentType = response.headers()["content-type"] ?? "";
  return contentType.includes("application/json") || contentType.includes("text/event-stream") || contentType.includes("text/plain");
}
async function waitForDomResponse(page) {
  const deadline = Date.now() + RESPONSE_TIMEOUT_MS;
  await page.waitForSelector(STOP_BUTTON_SELECTOR, { state: "visible", timeout: 15e3 }).catch(() => {
  });
  await page.waitForSelector(STOP_BUTTON_SELECTOR, { state: "hidden", timeout: RESPONSE_TIMEOUT_MS }).catch(() => {
  });
  while (Date.now() < deadline) {
    for (const selector of RESPONSE_SELECTORS) {
      const elements = await page.$$(selector);
      if (elements.length === 0) {
        continue;
      }
      const text = await elements[elements.length - 1].innerText().catch(() => "");
      if (text.trim()) {
        return text.trim();
      }
    }
    await page.waitForTimeout(1e3);
  }
  throw new Error("Copilot response not found in DOM");
}
async function uploadImage(page, imageB64) {
  const tmpPath = path.join(os.tmpdir(), `kiroku-${Date.now()}.png`);
  await fs.promises.writeFile(tmpPath, Buffer.from(imageB64, "base64"));
  const plusButton = await findFirstLocator(page, PLUS_BUTTON_SELECTORS);
  if (plusButton) {
    await plusButton.click({ timeout: 1e4 }).catch(() => {
    });
    await page.waitForTimeout(300);
  }
  const fileInput = await findFirstLocator(page, FILE_INPUT_SELECTORS);
  if (!fileInput) {
    throw new Error("Copilot upload input not found");
  }
  await fileInput.setInputFiles(tmpPath);
  await waitForAttachmentReady(page, path.basename(tmpPath));
  return tmpPath;
}
async function findFirstLocator(page, selectors) {
  for (const selector of selectors) {
    const locator = page.locator(selector).first();
    if (await locator.count() > 0) {
      return locator;
    }
  }
  return null;
}
async function waitForAttachmentReady(page, fileName) {
  const deadline = Date.now() + 15e3;
  while (Date.now() < deadline) {
    const hasAttachedFile = await page.locator('input[type="file"]').evaluateAll(
      (elements) => elements.some((element) => {
        if (!(element instanceof HTMLInputElement)) {
          return false;
        }
        return (element.files?.length ?? 0) > 0;
      })
    ).catch(() => false);
    if (hasAttachedFile) {
      return;
    }
    const fileNameVisible = await page.getByText(fileName, { exact: false }).first().isVisible().catch(() => false);
    if (fileNameVisible) {
      return;
    }
    for (const selector of ATTACHMENT_READY_SELECTORS) {
      const visible = await page.locator(selector).first().isVisible().catch(() => false);
      if (visible) {
        return;
      }
    }
    await page.waitForTimeout(250);
  }
  throw new Error("Copilot image attachment could not be confirmed");
}
function createServer2(session) {
  return http.createServer(async (req, res) => {
    try {
      if (req.method === "GET" && req.url === "/health") {
        return writeJson(res, 200, { status: "ok" });
      }
      if (req.method === "POST" && req.url === "/v1/chat/completions") {
        const payload = await readJsonBody(req);
        const prompt = parseOpenAiRequest(payload);
        const description = await session.describe(
          prompt.systemPrompt,
          prompt.userPrompt,
          prompt.imageB64
        );
        return writeJson(res, 200, {
          choices: [{ message: { role: "assistant", content: description } }]
        });
      }
      return writeJson(res, 404, { error: "Not found" });
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      console.error("[copilot] request failed", error);
      return writeJson(res, 500, { error: message });
    }
  });
}
async function readJsonBody(req) {
  const chunks = [];
  for await (const chunk of req) {
    chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk));
  }
  const body = Buffer.concat(chunks).toString("utf8");
  if (!body.trim()) {
    return {};
  }
  return JSON.parse(body);
}
function parseOpenAiRequest(payload) {
  const messages = payload.messages ?? [];
  const systemPrompt = messages.filter((message) => message.role === "system").map((message) => normalizeTextContent(message.content)).filter(Boolean).join("\n\n");
  let userPrompt = "";
  let imageB64;
  for (const message of messages) {
    if (message.role !== "user") {
      continue;
    }
    if (typeof message.content === "string") {
      userPrompt = `${userPrompt}
${message.content}`.trim();
      continue;
    }
    for (const part of message.content ?? []) {
      if (part.type === "text" && part.text) {
        userPrompt = `${userPrompt}
${part.text}`.trim();
      }
      if (part.type === "image_url" && part.image_url?.url) {
        imageB64 = extractBase64Payload(part.image_url.url);
      }
    }
  }
  if (!userPrompt.trim()) {
    throw new Error("User prompt is empty");
  }
  return { systemPrompt, userPrompt, imageB64 };
}
function normalizeTextContent(content) {
  if (typeof content === "string") {
    return content.trim();
  }
  return (content ?? []).filter((part) => part.type === "text" && part.text).map((part) => part.text?.trim() ?? "").filter(Boolean).join("\n");
}
function extractBase64Payload(dataUrl) {
  const match = dataUrl.match(/^data:[^;]+;base64,(.+)$/);
  return match ? match[1] : dataUrl;
}
function writeJson(res, statusCode, body) {
  const payload = JSON.stringify(body);
  res.writeHead(statusCode, {
    "Content-Type": "application/json",
    "Content-Length": Buffer.byteLength(payload)
  });
  res.end(payload);
}
function parseArgs(argv) {
  let port = DEFAULT_PORT;
  let cdpPort = DEFAULT_CDP_PORT;
  let help = false;
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--help" || arg === "-h") {
      help = true;
      continue;
    }
    if (arg === "--port") {
      port = parsePort(argv[index + 1], "--port");
      index += 1;
      continue;
    }
    if (arg === "--cdp-port") {
      cdpPort = parsePort(argv[index + 1], "--cdp-port");
      index += 1;
      continue;
    }
  }
  return { port, cdpPort, help };
}
function parsePort(value, flagName) {
  const port = Number(value);
  if (!Number.isInteger(port) || port < 1 || port > 65535) {
    throw new Error(`${flagName} must be a valid port number`);
  }
  return port;
}
var globalOptions = parseArgs(process.argv.slice(2));
async function main() {
  if (globalOptions.help) {
    console.error("Usage: node copilot_server.js [--port 18080] [--cdp-port 9222]");
    return;
  }
  const session = new CopilotSession();
  const server = createServer2(session);
  server.listen(globalOptions.port, "127.0.0.1", () => {
    console.error(
      `[copilot] listening on http://127.0.0.1:${globalOptions.port} (cdp:${globalOptions.cdpPort})`
    );
  });
  const shutdown = async () => {
    server.close();
    await session.close();
    process.exit(0);
  };
  process.on("SIGINT", () => void shutdown());
  process.on("SIGTERM", () => void shutdown());
}
main().catch((error) => {
  console.error("[copilot] fatal error", error);
  process.exit(1);
});
