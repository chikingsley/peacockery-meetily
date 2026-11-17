#!/usr/bin/env node
import { execFile } from "node:child_process";
import { randomUUID } from "node:crypto";
// GMK operator CLI. Credentials stay in the private store; output contains only the invitation.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs, parseEnv, promisify } from "node:util";

import {
  ACTIVATION,
  VOICE,
  SCOPES,
  identityKey,
  resolvePerson,
  manageAccess,
} from "./lib/access.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const privateDir = path.join(os.homedir(), ".local/share/peacockery-meetily");
const run = promisify(execFile);
const sql = (s) => `'${s.replaceAll("'", "''")}'`;
const help = `Usage: node scripts/access.mjs <lookup|invite|status|revoke> "Name" [--email address] [--new-device] [--json]
Names resolve against verified CICA contacts. --email selects a specific archived contact.
invite reuses the existing code; --new-device creates separate access for another computer.
revoke disables all credentials issued to this person by this operator.
`;

async function request(url, options = {}) {
  const response = await fetch(url, {
    ...options,
    redirect: "error",
    signal: AbortSignal.timeout(30000),
  });
  if (!response.ok)
    throw Error(
      `${new URL(url).hostname}${new URL(url).pathname} returned HTTP ${response.status}.`
    );
  return response;
}
async function archive(name, args) {
  const response = await request("http://127.0.0.1:8771/mcp", {
    method: "POST",
    headers: {
      "content-type": "application/json",
      accept: "application/json, text/event-stream",
    },
    body: JSON.stringify({
      jsonrpc: "2.0",
      id: randomUUID(),
      method: "tools/call",
      params: { name, arguments: args },
    }),
  });
  const text = await response.text();
  const data = response.headers
    .get("content-type")
    ?.includes("text/event-stream")
    ? JSON.parse(
        text
          .split("\n")
          .find((line) => line.startsWith("data: "))
          ?.slice(6) ?? "{}"
      )
    : JSON.parse(text);
  if (data.error || data.result?.isError)
    throw Error(
      "Communications archive lookup failed. Check the archive service."
    );
  return (
    data.result?.structuredContent ??
    JSON.parse(
      data.result?.content?.find((c) => c.type === "text")?.text ?? "{}"
    )
  );
}
async function query(statement) {
  let result;
  try {
    result = await run(
      "pnpm",
      [
        "--silent",
        "dlx",
        "wrangler@4.157.0",
        "d1",
        "execute",
        "bccfdd3b-b62c-4c8e-8c6d-749c3403b269",
        "--config",
        path.join(root, "services/activation/wrangler.jsonc"),
        "--remote",
        "--json",
        "--command",
        statement,
      ],
      { cwd: root, timeout: 120000, maxBuffer: 4 * 1024 * 1024 }
    );
  } catch {
    throw Error(
      "Voice account provisioning needs the operator’s Cloudflare login. Run pnpm dlx wrangler@4.157.0 login, then retry."
    );
  }
  const results = JSON.parse(result.stdout);
  if (results.some((r) => !r.success))
    throw Error("Voice account provisioning failed. Retry after checking D1.");
  return results.flatMap((r) => r.results ?? []);
}
export function services() {
  const voiceKey =
    process.env.PEACOCKERY_VOICE_ADMIN_API_KEY ??
    parseEnv(
      fs.readFileSync(
        path.resolve(root, "../peacockery-voice/.dev.vars"),
        "utf8"
      )
    ).PEACOCKERY_VOICE_ADMIN_API_KEY;
  if (!voiceKey)
    throw Error(
      "Configure PEACOCKERY_VOICE_ADMIN_API_KEY in the operator environment."
    );
  const activationKey = fs
    .readFileSync(path.join(privateDir, "activation-admin.key"), "utf8")
    .trim();
  const voice = async (route, method = "GET", body) =>
    (
      await request(`${VOICE}${route}`, {
        method,
        headers: {
          authorization: `Bearer ${voiceKey}`,
          "content-type": "application/json",
        },
        ...(body ? { body: JSON.stringify(body) } : {}),
      })
    ).json();
  const activation = async (route, body) =>
    (
      await request(`${ACTIVATION}${route}`, {
        method: "POST",
        headers: {
          authorization: `Bearer ${activationKey}`,
          "content-type": "application/json",
        },
        body: JSON.stringify(body),
      })
    ).json();
  return {
    credentials: async () => (await voice("/v1/admin/api-keys")).credentials,
    ensureUser: async (person) => {
      const find = `SELECT id FROM users WHERE lower(email) = ${sql(person.email)}`;
      const existing = await query(find);
      if (existing.length === 1) return existing[0].id;
      const id = `meetily-${identityKey(person.email).slice(0, 32)}`;
      const now = new Date().toISOString();
      const inserted = await query(
        `INSERT INTO users (id,email,display_name,created_at,updated_at) VALUES (${sql(id)},${sql(person.email)},${sql(person.name)},${sql(now)},${sql(now)}) ON CONFLICT(email) DO NOTHING; ${find}`
      );
      const user = inserted.find((r) => r.id);
      if (!user) throw Error("Voice user creation needs operator review.");
      return user.id;
    },
    createCredential: (user_id, label) =>
      voice("/v1/admin/api-keys", "POST", {
        environment: "lab",
        label,
        user_id,
        scopes: SCOPES,
      }),
    revokeCredential: (id) =>
      voice(`/v1/admin/api-keys/${encodeURIComponent(id)}`, "DELETE"),
    createCode: (payload) => activation("/admin/codes", payload),
    codeStatus: (code) => activation("/admin/status", { code }),
    revokeCode: (code) => activation("/admin/revoke", { code }),
  };
}

export function savePrivate(file, value) {
  const temporary = `${file}.${randomUUID()}.tmp`;
  const fd = fs.openSync(temporary, "wx", 0o600);
  try {
    fs.writeFileSync(fd, JSON.stringify(value, null, 2));
    fs.fsyncSync(fd);
  } finally {
    fs.closeSync(fd);
  }
  try {
    fs.renameSync(temporary, file);
  } finally {
    if (fs.existsSync(temporary)) fs.unlinkSync(temporary);
  }
}

async function main() {
  const { values, positionals } = parseArgs({
    allowPositionals: true,
    options: {
      email: { type: "string" },
      "new-device": { type: "boolean" },
      json: { type: "boolean" },
      help: { type: "boolean" },
    },
  });
  if (values.help) {
    console.log(help);
    return;
  }
  const [action, name] = positionals;
  if (
    !["lookup", "invite", "status", "revoke"].includes(action) ||
    !name ||
    positionals.length !== 2 ||
    (values["new-device"] && action !== "invite")
  )
    throw Error(help);
  const person = await resolvePerson(name, values.email, archive);
  if (action === "lookup") {
    console.log(JSON.stringify(person, null, 2));
    return;
  }
  const dir = path.join(privateDir, "invitations");
  fs.mkdirSync(dir, { recursive: true, mode: 0o700 });
  const file = path.join(dir, `${identityKey(person.email)}.json`);
  const lock = `${file}.lock`;
  let fd;
  try {
    fd = fs.openSync(lock, "wx", 0o600);
  } catch {
    throw Error(
      `An access operation holds ${lock}. Check its recorded PID before retrying.`
    );
  }
  fs.writeFileSync(fd, String(process.pid));
  try {
    const state = fs.existsSync(file)
      ? JSON.parse(fs.readFileSync(file, "utf8"))
      : null;
    const result = await manageAccess(
      { action, person, state, newDevice: values["new-device"] },
      services(),
      (value) => savePrivate(file, value)
    );
    if (values.json || action !== "invite") {
      console.log(JSON.stringify(result, null, 2));
      return;
    }
    console.log(
      `${person.name} <${person.email}>\nInstaller: ${result.installer}\nActivation code: ${result.code.slice(0, 4)} ${result.code.slice(4)}\nStatus: ${result.status}. Code expires ${result.expires_at}.\n${result.status === "activated" ? "This installation is activated. Use --new-device for another computer." : "Enter the code in Meetily. Access continues after activation."}`
    );
  } finally {
    fs.closeSync(fd);
    fs.unlinkSync(lock);
  }
}
if (import.meta.main)
  main().catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
