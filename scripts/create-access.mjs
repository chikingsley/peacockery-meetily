#!/usr/bin/env node
// Operator-only: mint revocable access and write a private recipient file.
import fs from "node:fs";
import path from "node:path";

const [recipient, destination, userId] = process.argv.slice(2);
const adminKey = process.env.PEACOCKERY_VOICE_ADMIN_API_KEY;
if (!recipient || !destination || !userId || !adminKey) {
  throw new Error("Usage: node --env-file=<private operator env> scripts/create-access.mjs <recipient> <private destination.peacockery-access> <existing-user-id>");
}
// Exclusive creation prevents losing an already issued credential.
const fd = fs.openSync(destination, "wx", 0o600);
try {
  const response = await fetch("https://voice-lab.peacockery.studio/v1/admin/api-keys", {
    method: "POST",
    headers: { authorization: `Bearer ${adminKey}`, "content-type": "application/json" },
    body: JSON.stringify({ environment: "lab", label: `Meetily: ${recipient}`, user_id: userId, scopes: ["models:read", "transcriptions:write", "transcriptions:read", "text:write"] }),
  });
  if (!response.ok) throw new Error(`Access issuance returned HTTP ${response.status}`);
  const result = await response.json();
  fs.writeFileSync(fd, JSON.stringify({ version: 1, recipient, base_url: "https://voice-lab.peacockery.studio", api_key: result.key }, null, 2));
  fs.writeFileSync(`${destination}.receipt.json`, JSON.stringify({ recipient, credential_id: result.credential_id, created: new Date().toISOString() }, null, 2), { mode: 0o600, flag: "wx" });
  console.log(`Created private activation: ${path.basename(destination)}. Credential ID: ${result.credential_id}`);
} catch (error) {
  if (fs.fstatSync(fd).size === 0) fs.unlinkSync(destination);
  throw error;
} finally { fs.closeSync(fd); }
