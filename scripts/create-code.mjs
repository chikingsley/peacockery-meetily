#!/usr/bin/env node
// Operator-only: exchange a private access record for a recipient's short code.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
const [source, destination] = process.argv.slice(2);
if (!source || !destination)
  throw Error(
    "Usage: node scripts/create-code.mjs <private access record> <private code receipt>"
  );
const fd = fs.openSync(destination, "wx", 0o600);
try {
  const key = fs
    .readFileSync(
      path.join(
        os.homedir(),
        ".local/share/peacockery-meetily/activation-admin.key"
      ),
      "utf8"
    )
    .trim();
  const payload = JSON.parse(fs.readFileSync(source, "utf8"));
  const response = await fetch(
    "https://meetily-activation.cheez2012.workers.dev/admin/codes",
    {
      method: "POST",
      headers: {
        authorization: `Bearer ${key}`,
        "content-type": "application/json",
      },
      body: JSON.stringify(payload),
    }
  );
  if (!response.ok)
    throw Error(`Code issuance returned HTTP ${response.status}`);
  const grant = await response.json();
  fs.writeFileSync(
    fd,
    JSON.stringify(
      {
        recipient: payload.recipient,
        ...grant,
        created: new Date().toISOString(),
      },
      null,
      2
    )
  );
  console.log(
    `Activation code for ${payload.recipient}: ${grant.code.slice(0, 4)} ${grant.code.slice(4)}. Valid for seven days and one installation.`
  );
} catch (error) {
  if (fs.fstatSync(fd).size === 0) fs.unlinkSync(destination);
  throw error;
} finally {
  fs.closeSync(fd);
}
