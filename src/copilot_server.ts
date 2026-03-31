import * as fs from "node:fs";
import * as http from "node:http";
import type { IncomingMessage, ServerResponse } from "node:http";
import * as os from "node:os";
import * as path from "node:path";

import {
  chromium,
  type Browser,
  type BrowserContext,
  type Page,
  type Response
} from "playwright";

const DEFAULT_PORT = 18080;
const DEFAULT_CDP_PORT = 9222;
const COPILOT_URL = "https://m365.cloud.microsoft/chat/";
const INPUT_SELECTOR = "#m365-chat-editor-target-element, [data-lexical-editor=\"true\"]";
const NEW_CHAT_BUTTON_SELECTOR = "[data-testid=\"newChatButton\"]";
const SEND_BUTTON_SELECTOR =
  ".fai-SendButton:not([disabled]), button[aria-label*=\"Send\"]:not([disabled]), button[aria-label*=\"送信\"]:not([disabled])";
const STOP_BUTTON_SELECTOR = ".fai-SendButton__stopBackground";
const PLUS_BUTTON_SELECTOR = "[data-testid=\"PlusMenuButton\"]";
const FILE_INPUT_SELECTOR = "[data-testid=\"uploadFileDialogInput\"]";
const RESPONSE_SELECTORS = [
  "[data-testid=\"markdown-reply\"]",
  "div[data-message-type=\"Chat\"]",
  "article[data-message-author-role=\"assistant\"]"
] as const;
const RESPONSE_URL_PATTERN =
  /substrate\.office\.com|copilot\.microsoft\.com|m365\.cloud\.microsoft|api\.bing\.microsoft\.com/i;
const RESPONSE_TIMEOUT_MS = 120_000;

type OpenAiRequestMessage = {
  role?: string;
  content?: string | Array<{ type?: string; text?: string; image_url?: { url?: string } }>;
};

type OpenAiRequest = {
  messages?: OpenAiRequestMessage[];
};

type ParsedPrompt = {
  systemPrompt: string;
  userPrompt: string;
  imageB64?: string;
};

class CopilotSession {
  private browser: Browser | null = null;
  private page: Page | null = null;
  private lock = false;

  async connect(cdpPort: number): Promise<void> {
    if (!this.browser || !this.browser.isConnected()) {
      this.browser = await chromium.connectOverCDP(`http://127.0.0.1:${cdpPort}`);
      this.browser.on("disconnected", () => {
        this.browser = null;
        this.page = null;
      });
    }

    this.page = await this.findOrCreateCopilotPage(this.browser);
  }

  async describe(systemPrompt: string, userPrompt: string, imageB64?: string): Promise<string> {
    if (this.lock) {
      throw new Error("Copilot session is busy");
    }

    this.lock = true;
    let uploadedImagePath: string | null = null;

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
          fs.promises.unlink(imagePathForCleanup).catch(() => {});
        }, 5_000);
      }
      this.lock = false;
    }
  }

  async close(): Promise<void> {
    if (this.browser) {
      await this.browser.close().catch(() => {});
    }
    this.browser = null;
    this.page = null;
    this.lock = false;
  }

  private async findOrCreateCopilotPage(browser: Browser): Promise<Page> {
    for (const context of browser.contexts()) {
      const existingPage = findCopilotPage(context);
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

  private async startNewChat(page: Page): Promise<void> {
    const newChatButton = page.locator(NEW_CHAT_BUTTON_SELECTOR).first();
    if ((await newChatButton.count()) > 0) {
      await newChatButton.click({ timeout: 10_000 }).catch(() => {});
      await page.waitForTimeout(500);
    }
  }
}

function findCopilotPage(context: BrowserContext): Page | null {
  for (const page of context.pages()) {
    if (page.url().includes("m365.cloud.microsoft/chat")) {
      return page;
    }
  }

  return null;
}

async function pastePrompt(page: Page, systemPrompt: string, userPrompt: string): Promise<void> {
  const fullPrompt = systemPrompt ? `${systemPrompt}\n\n${userPrompt}` : userPrompt;
  const inputEl = await page.waitForSelector(INPUT_SELECTOR, {
    state: "visible",
    timeout: 10_000
  });

  await inputEl.click();
  await page.keyboard.press(process.platform === "darwin" ? "Meta+A" : "Control+A").catch(() => {});
  await page.keyboard.press("Backspace").catch(() => {});

  await page.evaluate((text: string) => {
    const el =
      document.querySelector("#m365-chat-editor-target-element") ??
      document.querySelector("[data-lexical-editor=\"true\"]");
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

async function submitPrompt(page: Page): Promise<string> {
  const responsePromise = page
    .waitForResponse(
      (candidate: Response) =>
        RESPONSE_URL_PATTERN.test(candidate.url()) &&
        candidate.status() === 200 &&
        candidate.request().method() === "POST" &&
        isLikelyCopilotCompletion(candidate),
      { timeout: RESPONSE_TIMEOUT_MS }
    )
    .catch(() => {});

  await page.locator(SEND_BUTTON_SELECTOR).first().click({ timeout: 10_000 });
  await responsePromise;

  return await waitForDomResponse(page);
}

function isLikelyCopilotCompletion(response: Response): boolean {
  const contentType = response.headers()["content-type"] ?? "";
  return (
    contentType.includes("application/json") ||
    contentType.includes("text/event-stream") ||
    contentType.includes("text/plain")
  );
}

async function waitForDomResponse(page: Page): Promise<string> {
  const deadline = Date.now() + RESPONSE_TIMEOUT_MS;

  await page.waitForSelector(STOP_BUTTON_SELECTOR, { state: "visible", timeout: 15_000 }).catch(() => {});
  await page
    .waitForSelector(STOP_BUTTON_SELECTOR, { state: "hidden", timeout: RESPONSE_TIMEOUT_MS })
    .catch(() => {});

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

    await page.waitForTimeout(1_000);
  }

  throw new Error("Copilot response not found in DOM");
}

async function uploadImage(page: Page, imageB64: string): Promise<string> {
  const tmpPath = path.join(os.tmpdir(), `kiroku-${Date.now()}.png`);
  await fs.promises.writeFile(tmpPath, Buffer.from(imageB64, "base64"));

  const plusButton = page.locator(PLUS_BUTTON_SELECTOR).first();
  if ((await plusButton.count()) > 0) {
    await plusButton.click({ timeout: 10_000 }).catch(() => {});
    await page.waitForTimeout(300);
  }

  const fileInput = page.locator(FILE_INPUT_SELECTOR).first();
  if ((await fileInput.count()) > 0) {
    await fileInput.setInputFiles(tmpPath);
    await page.waitForTimeout(1_000);
  } else {
    console.error("[copilot] file upload input not found, proceeding without image");
  }

  return tmpPath;
}

function createServer(session: CopilotSession): http.Server {
  return http.createServer(async (req: IncomingMessage, res: ServerResponse) => {
    try {
      if (req.method === "GET" && req.url === "/health") {
        return writeJson(res, 200, { status: "ok" });
      }

      if (req.method === "POST" && req.url === "/v1/chat/completions") {
        const payload = (await readJsonBody(req)) as OpenAiRequest;
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

async function readJsonBody(req: IncomingMessage): Promise<unknown> {
  const chunks: Buffer[] = [];

  for await (const chunk of req) {
    chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk));
  }

  const body = Buffer.concat(chunks).toString("utf8");
  if (!body.trim()) {
    return {};
  }

  return JSON.parse(body);
}

function parseOpenAiRequest(payload: OpenAiRequest): ParsedPrompt {
  const messages = payload.messages ?? [];
  const systemPrompt = messages
    .filter((message) => message.role === "system")
    .map((message) => normalizeTextContent(message.content))
    .filter(Boolean)
    .join("\n\n");

  let userPrompt = "";
  let imageB64: string | undefined;

  for (const message of messages) {
    if (message.role !== "user") {
      continue;
    }

    if (typeof message.content === "string") {
      userPrompt = `${userPrompt}\n${message.content}`.trim();
      continue;
    }

    for (const part of message.content ?? []) {
      if (part.type === "text" && part.text) {
        userPrompt = `${userPrompt}\n${part.text}`.trim();
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

function normalizeTextContent(content: OpenAiRequestMessage["content"]): string {
  if (typeof content === "string") {
    return content.trim();
  }

  return (content ?? [])
    .filter((part) => part.type === "text" && part.text)
    .map((part) => part.text?.trim() ?? "")
    .filter(Boolean)
    .join("\n");
}

function extractBase64Payload(dataUrl: string): string {
  const match = dataUrl.match(/^data:[^;]+;base64,(.+)$/);
  return match ? match[1] : dataUrl;
}

function writeJson(res: ServerResponse, statusCode: number, body: unknown): void {
  const payload = JSON.stringify(body);
  res.writeHead(statusCode, {
    "Content-Type": "application/json",
    "Content-Length": Buffer.byteLength(payload)
  });
  res.end(payload);
}

type ParsedArgs = {
  port: number;
  cdpPort: number;
  help: boolean;
};

function parseArgs(argv: string[]): ParsedArgs {
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

function parsePort(value: string | undefined, flagName: string): number {
  const port = Number(value);
  if (!Number.isInteger(port) || port < 1 || port > 65_535) {
    throw new Error(`${flagName} must be a valid port number`);
  }
  return port;
}

const globalOptions = parseArgs(process.argv.slice(2));

async function main(): Promise<void> {
  if (globalOptions.help) {
    console.error("Usage: node copilot_server.js [--port 18080] [--cdp-port 9222]");
    return;
  }

  const session = new CopilotSession();
  const server = createServer(session);

  server.listen(globalOptions.port, "127.0.0.1", () => {
    console.error(
      `[copilot] listening on http://127.0.0.1:${globalOptions.port} (cdp:${globalOptions.cdpPort})`
    );
  });

  const shutdown = async (): Promise<void> => {
    server.close();
    await session.close();
    process.exit(0);
  };

  process.on("SIGINT", () => void shutdown());
  process.on("SIGTERM", () => void shutdown());
}

main().catch((error: unknown) => {
  console.error("[copilot] fatal error", error);
  process.exit(1);
});
