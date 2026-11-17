import assert from "node:assert/strict";
import test from "node:test";

import { manageAccess, resolvePerson } from "./lib/access.mjs";
const person = {
  name: "Test Person",
  email: "test@cicahuntek.us",
  source: "record-1",
};
function fixture() {
  let state = null,
    count = 0,
    codeCount = 0;
  const credentials = [],
    grants = new Map();
  const api = {
    credentials: async () => credentials,
    ensureUser: async () => "user-1",
    createCredential: async (user_id) => {
      const c = {
        credential_id: `key-${++count}`,
        user_id,
        status: "active",
        key: `pv_lab_secret_${count}`,
      };
      credentials.push(c);
      return c;
    },
    revokeCredential: async (id) => {
      credentials.find((c) => c.credential_id === id).status = "revoked";
    },
    createCode: async () => {
      const code = String(++codeCount).padStart(8, "0");
      grants.set(code, { status: "ready", expires_at: "2026-12-01" });
      return { code };
    },
    codeStatus: async (code) => grants.get(code) ?? { status: "unavailable" },
    revokeCode: async (code) => {
      grants.delete(code);
    },
  };
  return {
    api,
    grants,
    credentials,
    state: () => state,
    run: (action, newDevice = false) =>
      manageAccess({ action, person, state, newDevice }, api, async (s) => {
        state = structuredClone(s);
      }),
  };
}
test("repeat invites preserve one account, key and invitation; output excludes keys", async () => {
  const f = fixture();
  const first = await f.run("invite"),
    second = await f.run("invite");
  assert.equal(first.code, second.code);
  assert.equal(f.credentials.length, 1);
  assert.equal(JSON.stringify(second).includes("pv_lab_"), false);
  f.grants.set(first.code, { status: "activated" });
  assert.equal((await f.run("invite")).status, "activated");
  assert.equal(f.credentials.length, 1);
});
test("new computers get independent keys and person revocation removes each", async () => {
  const f = fixture();
  const first = await f.run("invite"),
    second = await f.run("invite", true);
  assert.notEqual(first.credential_id, second.credential_id);
  assert.notEqual(first.code, second.code);
  assert.equal(f.credentials.length, 2);
  await f.run("revoke");
  assert.ok(f.credentials.every((c) => c.status === "revoked"));
  assert.equal(f.grants.size, 0);
  assert.ok(
    (await f.run("status")).devices.every((d) => d.status === "revoked")
  );
});
test("expired codes renew while preserving the credential", async () => {
  const f = fixture();
  const first = await f.run("invite");
  f.grants.delete(first.code);
  const next = await f.run("invite");
  assert.notEqual(first.code, next.code);
  assert.equal(f.credentials.length, 1);
});
test("code service failure resumes from the privately stored key", async () => {
  const f = fixture();
  const create = f.api.createCode;
  f.api.createCode = async () => {
    throw Error("offline");
  };
  await assert.rejects(f.run("invite"), /offline/);
  assert.equal(f.credentials.length, 1);
  f.api.createCode = create;
  await f.run("invite");
  assert.equal(f.credentials.length, 1);
});
test("failed credential persistence revokes the issued credential", async () => {
  const f = fixture();
  await assert.rejects(
    manageAccess({ action: "invite", person }, f.api, async () => {
      throw Error("disk full");
    }),
    /disk full/
  );
  assert.equal(f.credentials[0].status, "revoked");
});
const contact = (title, email, id = email) => ({
  id,
  title,
  active: 1,
  state: "ready",
  observed: 100,
  metadata: { emails: [email] },
});
test("lookup paginates and deduplicates source copies by verified address", async () => {
  const calls = [];
  const archive = async (name, args) => {
    calls.push(args);
    if (name === "context_read") return contact("Test Person", person.email);
    return args.offset === 0
      ? {
          results: [
            contact("Test Person", person.email),
            contact("Someone Else", "other@cicahuntek.us"),
          ],
          has_more: true,
          next_offset: 100,
        }
      : {
          results: [contact("Test Person", person.email, "duplicate")],
          has_more: false,
        };
  };
  assert.equal(
    (await resolvePerson("Test", undefined, archive)).email,
    person.email
  );
  assert.equal(calls.length, 3);
});
test("ambiguous names require the exact verified email; body mentions are ignored", async () => {
  const archive = async (name, args) =>
    name === "context_read"
      ? contact("Test Person", person.email)
      : {
          results: [
            contact("Test Person", person.email),
            contact("Test Other", "other@cicahuntek.us"),
            contact("Body Mention", "third@cicahuntek.us"),
          ],
          has_more: false,
        };
  await assert.rejects(
    resolvePerson("Test", undefined, archive),
    /Choose the person/
  );
  assert.equal(
    (await resolvePerson("Test", person.email, archive)).email,
    person.email
  );
  await assert.rejects(
    resolvePerson("Test", "guess@cicahuntek.us", archive),
    /verified work contact/
  );
});
