const json = (body, status = 200) =>
  Response.json(body, { status, headers: { "cache-control": "no-store" } });
const digest = async (s) =>
  [
    ...new Uint8Array(
      await crypto.subtle.digest("SHA-256", new TextEncoder().encode(s))
    ),
  ]
    .map((x) => x.toString(16).padStart(2, "0"))
    .join("");
export class ActivationState {
  constructor(ctx) {
    this.ctx = ctx;
  }
  async fetch(request) {
    const { action, ...body } = await request.json();
    return this.ctx.storage.transaction(async (store) => {
      const now = Date.now();
      if (action === "limit") {
        let value = await store.get("limit");
        if (!value || now >= value.until)
          value = { count: 0, until: now + 900000 };
        value.count++;
        await store.put("limit", value);
        await store.setAlarm(value.until);
        return json({}, value.count > 10 ? 429 : 200);
      }
      if (action === "revoke") {
        await store.delete("grant");
        return json({ revoked: true });
      }
      if (action === "create") {
        if (await store.get("grant")) return json({}, 409);
        await store.put("grant", {
          payload: body.payload,
          until: now + 7 * 86400000,
          installation: null,
        });
        await store.setAlarm(now + 7 * 86400000);
        return json({}, 201);
      }
      const grant = await store.get("grant");
      if (
        !grant ||
        grant.until <= now ||
        (grant.installation && grant.installation !== body.installation)
      )
        return json({ error: "Check the code or request a new one." }, 400);
      grant.installation = body.installation;
      await store.put("grant", grant);
      return json(grant.payload);
    });
  }
  async alarm() {
    await this.ctx.storage.deleteAll();
  }
}
const call = (env, name, body) =>
  env.STATE.get(env.STATE.idFromName(name)).fetch("https://state/", {
    method: "POST",
    body: JSON.stringify(body),
  });
export default {
  async fetch(request, env) {
    if (request.method !== "POST") return json({ error: "Use POST" }, 405);
    const path = new URL(request.url).pathname;
    if (
      path.startsWith("/admin/") &&
      (!env.ADMIN_KEY ||
        request.headers.get("authorization") !== `Bearer ${env.ADMIN_KEY}`)
    )
      return json({}, 403);
    if (Number(request.headers.get("content-length") ?? 0) > 16384)
      return json({}, 413);
    const raw = await request.text();
    if (raw.length > 16384) return json({}, 413);
    let body;
    try {
      body = JSON.parse(raw);
    } catch {
      return json({}, 400);
    }
    if (!body || typeof body !== "object" || Array.isArray(body))
      return json({}, 400);
    if (path === "/admin/revoke") {
      if (!/^\d{8}$/.test(body.code ?? "")) return json({}, 400);
      return call(env, "code:" + (await digest(body.code)), {
        action: "revoke",
      });
    }
    if (path === "/admin/codes") {
      if (
        body.version !== 1 ||
        typeof body.recipient !== "string" ||
        !body.recipient.trim() ||
        body.base_url !== "https://voice-lab.peacockery.studio" ||
        !/^pv_lab_[a-zA-Z0-9_-]+$/.test(body.api_key ?? "")
      )
        return json({}, 400);
      const payload = {
        version: 1,
        recipient: body.recipient,
        base_url: body.base_url,
        api_key: body.api_key,
      };
      for (let i = 0; i < 10; i++) {
        const values = crypto.getRandomValues(new Uint32Array(1));
        if (values[0] >= 4200000000) continue;
        const code = String(values[0] % 100000000).padStart(8, "0");
        const result = await call(env, "code:" + (await digest(code)), {
          action: "create",
          payload,
        });
        if (result.status === 201)
          return json({ code, expires_in_days: 7 }, 201);
      }
      return json({}, 503);
    }
    if (path !== "/redeem") return json({}, 404);
    const ip = request.headers.get("cf-connecting-ip");
    if (!ip) return json({}, 400);
    const limit = await call(env, "ip:" + (await digest(ip)), {
      action: "limit",
    });
    if (limit.status === 429)
      return json(
        { error: "Too many attempts. Try again in 15 minutes." },
        429
      );
    if (
      !/^\d{8}$/.test(body.code ?? "") ||
      !/^[a-zA-Z0-9-]{20,80}$/.test(body.installation ?? "")
    )
      return json({ error: "Enter the eight-digit activation code." }, 400);
    return call(env, "code:" + (await digest(body.code)), {
      action: "redeem",
      installation: body.installation,
    });
  },
};
