import { createHash, randomUUID } from "node:crypto";

export const VOICE = "https://voice-lab.peacockery.studio";
export const ACTIVATION = "https://meetily-activation.cheez2012.workers.dev";
export const INSTALLER =
  "https://github.com/chikingsley/peacockery-meetily/releases/latest/download/Peacockery-Meetily-Setup.exe";
export const SCOPES = [
  "models:read",
  "transcriptions:write",
  "transcriptions:read",
  "text:write",
];
export const identityKey = (email) =>
  createHash("sha256").update(email.toLowerCase()).digest("hex");
const words = (s) => s.toLowerCase().match(/[\p{L}\p{N}]+/gu) ?? [];

export async function resolvePerson(query, email, archive) {
  const terms = words(query);
  if (!terms.length) throw Error("Supply a person’s name.");
  const candidates = new Map();
  let offset = 0;
  do {
    const page = await archive("context_search", {
      query: terms.join(" "),
      kind: "contact",
      limit: 100,
      offset,
      ...(email ? {} : { domain: "cicahuntek.us" }),
    });
    for (const contact of page.results ?? []) {
      if (
        !contact.active ||
        contact.state !== "ready" ||
        !terms.every((term) => words(contact.title).includes(term))
      )
        continue;
      for (const address of contact.metadata?.emails ?? []) {
        const normalized = address.toLowerCase();
        if (
          email
            ? normalized !== email.toLowerCase()
            : !normalized.endsWith("@cicahuntek.us")
        )
          continue;
        const previous = candidates.get(normalized);
        if (!previous || contact.observed > previous.observed) {
          candidates.set(normalized, {
            name: contact.title,
            email: normalized,
            source: contact.id,
            observed: contact.observed,
          });
        }
      }
    }
    if (
      page.has_more &&
      (!Number.isInteger(page.next_offset) || page.next_offset <= offset)
    ) {
      throw Error(
        "Contact search returned incomplete pagination. Retry lookup."
      );
    }
    offset = page.has_more ? page.next_offset : null;
    if (offset >= 10000)
      throw Error("Use a fuller name to narrow contact lookup.");
  } while (offset !== null);
  if (candidates.size !== 1) {
    const choices = [...candidates.values()]
      .map((p) => `${p.name} <${p.email}>`)
      .join("; ");
    throw Error(
      choices
        ? `Choose the person with --email: ${choices}`
        : "Look up the person’s verified work contact in the communications archive, then retry with their full name and --email."
    );
  }
  const person = [...candidates.values()][0];
  const record = await archive("context_read", {
    record_id: person.source,
    max_chars: 4000,
  });
  if (
    !record.active ||
    record.state !== "ready" ||
    !record.metadata?.emails?.some((e) => e.toLowerCase() === person.email)
  ) {
    throw Error("Refresh the person’s contact record before issuing access.");
  }
  return person;
}

export async function manageAccess(
  { action, person, state, newDevice = false },
  api,
  save
) {
  state ??= { version: 1, person, user_id: null, devices: [] };
  if (state.version !== 1 || state.person.email !== person.email)
    throw Error("Access record identity needs operator review.");
  const credentials = await api.credentials();
  const active = (device) =>
    credentials.some(
      (c) =>
        c.credential_id === device.credential_id &&
        c.status === "active" &&
        c.user_id === state.user_id
    );
  if (action === "status") {
    const devices = [];
    for (const device of state.devices) {
      devices.push({
        credential_id: device.credential_id,
        created: device.created,
        ...(active(device)
          ? device.code
            ? await api.codeStatus(device.code)
            : { status: "pending-code" }
          : { status: "revoked" }),
      });
    }
    return { person, devices };
  }
  if (action === "revoke") {
    for (const device of state.devices) {
      if (active(device)) await api.revokeCredential(device.credential_id);
      // Persist credential revocation before invitation cleanup; a retry finishes cleanup.
      device.revoked_at ??= new Date().toISOString();
      await save(state);
      if (device.code) await api.revokeCode(device.code);
    }
    return { person, status: "revoked", devices: state.devices.length };
  }
  state.user_id ??= await api.ensureUser(person);
  let device = newDevice ? null : state.devices.findLast(active);
  if (!device) {
    const label = `Meetily: ${person.name.slice(0, 65)} (${randomUUID()})`;
    const created = await api.createCredential(state.user_id, label);
    device = {
      credential_id: created.credential_id,
      key: created.key,
      created: new Date().toISOString(),
    };
    state.devices.push(device);
    try {
      await save(state);
    } catch (error) {
      await api.revokeCredential(device.credential_id);
      throw error;
    }
  }
  let grant = device.code
    ? await api.codeStatus(device.code)
    : { status: "unavailable" };
  if (grant.status === "unavailable") {
    const issued = await api.createCode({
      version: 1,
      recipient: person.name,
      base_url: VOICE,
      api_key: device.key,
    });
    device.code = issued.code;
    try {
      await save(state);
    } catch (error) {
      await api.revokeCode(device.code);
      throw error;
    }
    grant = await api.codeStatus(device.code);
  }
  if (!["ready", "activated"].includes(grant.status))
    throw Error("Invitation status needs operator review.");
  return {
    person,
    credential_id: device.credential_id,
    code: device.code,
    ...grant,
    installer: INSTALLER,
  };
}
