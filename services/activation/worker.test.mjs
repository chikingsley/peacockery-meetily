import assert from "node:assert/strict";
import test from "node:test";

import worker, { ActivationState } from "./worker.mjs";
function environment() {
  const objects = new Map();
  return {
    ADMIN_KEY: "operator-test-key",
    STATE: {
      idFromName: (s) => s,
      get(name) {
        if (!objects.has(name)) {
          const values = new Map();
          let queue = Promise.resolve();
          const storage = {
            get: async (k) => structuredClone(values.get(k)),
            put: async (k, v) => values.set(k, structuredClone(v)),
            delete: async (k) => values.delete(k),
            setAlarm: async () => {},
            deleteAll: async () => values.clear(),
            transaction(fn) {
              const next = queue.then(() => fn(storage));
              queue = next.catch(() => {});
              return next;
            },
          };
          objects.set(name, new ActivationState({ storage }));
        }
        return {
          fetch: (url, init) => objects.get(name).fetch(new Request(url, init)),
        };
      },
    },
  };
}
const payload = {
  version: 1,
  recipient: "Recipient",
  base_url: "https://voice-lab.peacockery.studio",
  api_key: "pv_lab_test_secret",
};
const post = (env, path, body, admin = false) =>
  worker.fetch(
    new Request("https://activation.test" + path, {
      method: "POST",
      headers: {
        "cf-connecting-ip": "192.0.2.1",
        ...(admin ? { authorization: "Bearer " + env.ADMIN_KEY } : {}),
      },
      body: JSON.stringify(body),
    }),
    env
  );
async function issue(env) {
  const r = await post(env, "/admin/codes", payload, true);
  assert.equal(r.status, 201);
  return (await r.json()).code;
}
test("issuance requires operator access and validates grants", async () => {
  const env = environment();
  assert.equal((await post(env, "/admin/codes", payload)).status, 403);
  for (const body of [
    null,
    [],
    {},
    { ...payload, api_key: "pv_admin_test" },
    { ...payload, base_url: "https://other.test" },
  ])
    assert.equal((await post(env, "/admin/codes", body, true)).status, 400);
  assert.match(await issue(env), /^\d{8}$/);
});
test("a concurrent claim has one winner, and that installation can retry", async () => {
  const env = environment(),
    code = await issue(env);
  const a = { code, installation: "11111111-1111-1111-1111-111111111111" },
    b = { code, installation: "22222222-2222-2222-2222-222222222222" };
  const results = await Promise.all([
    post(env, "/redeem", a),
    post(env, "/redeem", b),
  ]);
  assert.deepEqual(results.map((r) => r.status).sort(), [200, 400]);
  const winner = results.findIndex((r) => r.status === 200),
    claim = [a, b][winner];
  assert.deepEqual(await results[winner].json(), payload);
  assert.equal((await post(env, "/redeem", claim)).status, 200);
  assert.equal((await post(env, "/admin/revoke", { code }, true)).status, 200);
  assert.equal((await post(env, "/redeem", claim)).status, 400);
});
test("expired grants fail and guessed codes are rate limited", async () => {
  const env = environment(),
    code = await issue(env),
    original = Date.now;
  try {
    Date.now = () => original() + 8 * 86400000;
    assert.equal(
      (
        await post(env, "/redeem", {
          code,
          installation: "33333333-3333-3333-3333-333333333333",
        })
      ).status,
      400
    );
  } finally {
    Date.now = original;
  }
  const other = environment();
  for (let i = 0; i < 10; i++)
    assert.equal(
      (
        await post(other, "/redeem", {
          code: "00000000",
          installation: "33333333-3333-3333-3333-333333333333",
        })
      ).status,
      400
    );
  assert.equal(
    (
      await post(other, "/redeem", {
        code: "00000000",
        installation: "33333333-3333-3333-3333-333333333333",
      })
    ).status,
    429
  );
});
test("operator status is private, secret-free and preserves the invitation", async () => {
  const env = environment(),
    code = await issue(env);
  assert.equal((await post(env, "/admin/status", { code })).status, 403);
  const ready = await (await post(env, "/admin/status", { code }, true)).json();
  assert.equal(ready.status, "ready");
  assert.ok(ready.expires_at);
  assert.equal(JSON.stringify(ready).includes("pv_lab_"), false);
  assert.equal(
    (
      await post(env, "/redeem", {
        code,
        installation: "11111111-1111-1111-1111-111111111111",
      })
    ).status,
    200
  );
  assert.equal(
    (await (await post(env, "/admin/status", { code }, true)).json()).status,
    "activated"
  );
  await post(env, "/admin/revoke", { code }, true);
  assert.equal(
    (await (await post(env, "/admin/status", { code }, true)).json()).status,
    "unavailable"
  );
});
