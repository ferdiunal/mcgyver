// The few addresses the page itself can't answer: the app's daily update check with its usage
// ping (/api/ping; /api/latest for a machine without an ID), and downloads from a GitHub
// repository that is still private.
// wrangler.jsonc routes only /api/* and /download/* here; every other path is a static asset.

const REPO = "ferdiunal/mcgyver";
// Where the app sends people for a new version: the page's download section.
const DOWNLOAD_PAGE = "https://mcgyver.ferdiunal.tr/?utm_source=mcgyver#download";
// Each download name and how its file is named in a release.
const DOWNLOADS = {
  "windows-x64": (name) => name.endsWith("-windows-x64.zip"),
  "windows-arm64": (name) => name.endsWith("-windows-arm64.zip"),
  deb: (name) => name.endsWith("_amd64.deb"),
  rpm: (name) => name.endsWith(".x86_64.rpm"),
  macos: (name) => name.endsWith(".dmg"),
  "macos-arm64": (name) => name.endsWith("-arm64.dmg"),
};
// GitHub is asked about the latest release at most this often; the rest come from the cache.
const RELEASE_TTL = 600;
// A failed lookup is kept this long, so a GitHub outage isn't asked about by every request.
const FAILED_TTL = 60;
// GitHub's signed address for a file lasts about five minutes; one is handed out for this long.
const ASSET_TTL = 90;
const PING_MAX_BYTES = 2048;

export default {
  async fetch(request, env, ctx) {
    const { pathname } = new URL(request.url);
    if (pathname === "/api/latest") {
      if (request.method !== "GET" && request.method !== "HEAD") return text("Method not allowed\n", 405, { Allow: "GET, HEAD" });
      return latest(await release(request, env, ctx));
    }
    if (pathname === "/api/ping") {
      if (request.method !== "POST") return text("Method not allowed\n", 405, { Allow: "POST" });
      return ping(request, env, ctx);
    }
    if (pathname.startsWith("/download/")) {
      const name = pathname.slice("/download/".length);
      // Own keys only: `__proto__` or `constructor` would otherwise be found on every object.
      const match = Object.hasOwn(DOWNLOADS, name) ? DOWNLOADS[name] : undefined;
      if (!match || (request.method !== "GET" && request.method !== "HEAD")) return env.ASSETS.fetch(request);
      return download(request, env, ctx, match);
    }
    return text("Not found\n", 404);
  },
};

function text(body, status = 200, headers = {}) {
  return new Response(body, {
    status,
    headers: { "Content-Type": "text/plain; charset=utf-8", "Cache-Control": "no-store", "X-Content-Type-Options": "nosniff", ...headers },
  });
}

// The app reads `key = value` lines and skips keys it doesn't know. Without a release to name,
// it still gets the page to look at. When the release can't be read at all, the answer is an
// error, which the app doesn't keep: it asks again within the hour instead of a day later.
function latest(found) {
  if (!found) return text("Try again later\n", 503, { "Retry-After": "3600" });
  const lines = [];
  if (found?.tag) lines.push(`latest = ${found.tag.replace(/^v/, "")}`);
  lines.push(`url = ${DOWNLOAD_PAGE}`);
  return text(`${lines.join("\n")}\n`);
}

async function ping(request, env, ctx) {
  if (Number(request.headers.get("Content-Length")) > PING_MAX_BYTES) return text("Too large\n", 413);
  const body = await readAtMost(request, PING_MAX_BYTES);
  if (!body) return text("Too large\n", 413);
  const form = new URLSearchParams(new TextDecoder().decode(body));

  const machine = form.get("machine") ?? "";
  const version = form.get("version") ?? "";
  const os = form.get("os") ?? "";
  if (!/^[0-9a-f]{64}$/.test(machine) || !/^[0-9A-Za-z.+-]{1,32}$/.test(version) || !["linux", "macos", "windows"].includes(os)) {
    return text("Bad request\n", 400);
  }
  // An optional field that doesn't fit is left empty rather than turning the ping away: one odd
  // value shouldn't cost the update check its answer.
  // Kept as text to show: no control characters, and cut between characters, not inside one.
  const osVersion = [...(form.get("os_version") ?? "").replace(/[\u0000-\u001f\u007f]/g, "").trim()].slice(0, 100).join("") || null;
  const arch = optional(form.get("arch"), /^[a-z0-9_]{1,16}$/);
  const lang = optional(form.get("lang"), /^[a-z-]{1,8}$/);
  // This zone leaves CF-Connecting-IP out (Cloudflare's "Remove visitor IP headers"); the runtime
  // still sets X-Real-IP.
  const ip = optional(request.headers.get("CF-Connecting-IP") ?? request.headers.get("X-Real-IP"), /^[0-9a-f:.]{2,45}$/);
  const country = request.cf?.country ?? null;
  const now = new Date().toISOString();

  const write = env.DB.batch([
    env.DB.prepare(
      `INSERT INTO machines (machine, first_seen, last_seen, version, os, os_version, arch, lang, ip, country, pings)
       VALUES (?1, ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1)
       ON CONFLICT (machine) DO UPDATE SET
         last_seen = excluded.last_seen, version = excluded.version, os = excluded.os,
         os_version = excluded.os_version, arch = excluded.arch, lang = excluded.lang,
         ip = excluded.ip, country = excluded.country, pings = pings + 1`,
    ).bind(machine, now, version, os, osVersion, arch, lang, ip, country),
    env.DB.prepare("INSERT OR IGNORE INTO days (day, machine, version) VALUES (?1, ?2, ?3)").bind(now.slice(0, 10), machine, version),
  ]);
  // A failed write loses one count; the app still hears about the new version.
  const [, found] = await Promise.all([write.catch((e) => console.error("ping: D1 write failed:", e.message)), release(request, env, ctx)]);
  return latest(found);
}

// The body, or null once it passes `max` bytes: a chunked one has no length to check first.
async function readAtMost(request, max) {
  if (!request.body) return new Uint8Array();
  const reader = request.body.getReader();
  const chunks = [];
  let size = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    size += value.byteLength;
    if (size > max) {
      await reader.cancel();
      return null;
    }
    chunks.push(value);
  }
  const body = new Uint8Array(size);
  let at = 0;
  for (const chunk of chunks) {
    body.set(chunk, at);
    at += chunk.byteLength;
  }
  return body;
}

function optional(value, pattern) {
  const v = (value ?? "").trim().toLowerCase();
  return pattern.test(v) ? v : null;
}

// GitHub's signed address for the file is good for a few minutes only, so the visitor is sent
// there straight away and nothing keeps it. Whatever goes wrong, they land on the download section.
async function download(request, env, ctx, match) {
  const fallback = new Response(null, { status: 302, headers: { Location: new URL("/#download", request.url).href, "Cache-Control": "no-store" } });
  const assets = (await release(request, env, ctx))?.assets ?? [];
  const candidates = assets.filter((a) => match(a.name));
  // A Mac build that runs on both Apple Silicon and Intel wins over one that doesn't.
  const asset = candidates.find((a) => a.name.includes("universal")) ?? candidates[0];
  if (!asset || !env.GITHUB_TOKEN) return fallback;
  const redirect = (location) => new Response(null, { status: 302, headers: { Location: location, "Cache-Control": "no-store" } });
  // The same signed address serves everyone for a minute and a half, well inside its five.
  const key = new Request(new URL(`/api/_cache/asset-${asset.id}`, request.url));
  const hit = await caches.default.match(key);
  if (hit) return redirect(await hit.text());
  try {
    const res = await fetch(`https://api.github.com/repos/${REPO}/releases/assets/${asset.id}`, {
      headers: { ...github(env), Accept: "application/octet-stream" },
      redirect: "manual",
    });
    const location = res.headers.get("Location");
    if (res.status < 300 || res.status > 399 || !location) {
      console.error(`download: GitHub answered ${res.status} for asset ${asset.id}`);
      return fallback;
    }
    ctx.waitUntil(caches.default.put(key, new Response(location, { headers: { "Cache-Control": `max-age=${ASSET_TTL}` } })));
    return redirect(location);
  } catch (e) {
    console.error("download: GitHub unreachable:", e.message);
    return fallback;
  }
}

function github(env) {
  return {
    Authorization: `Bearer ${env.GITHUB_TOKEN}`,
    Accept: "application/vnd.github+json",
    "X-GitHub-Api-Version": "2022-11-28",
    "User-Agent": "mcgyver-site",
  };
}

// The newest published release as { tag, assets: [{ id, name }] }, or null when it can't be
// read. "No release yet" is an answer too and is kept as long; a failure is kept for a minute.
async function release(request, env, ctx) {
  const key = new Request(new URL("/api/_cache/release-latest", request.url));
  const cache = caches.default;
  const hit = await cache.match(key);
  if (hit) {
    const kept = await hit.json();
    return kept.failed ? null : kept;
  }
  if (!env.GITHUB_TOKEN) return null;
  const failed = () => {
    const stored = new Response(JSON.stringify({ failed: true }), {
      headers: { "Content-Type": "application/json", "Cache-Control": `max-age=${FAILED_TTL}` },
    });
    ctx.waitUntil(cache.put(key, stored));
    return null;
  };

  let found;
  try {
    const res = await fetch(`https://api.github.com/repos/${REPO}/releases/latest`, { headers: github(env) });
    if (res.status === 404) {
      found = { tag: null, assets: [] };
    } else if (res.ok) {
      const data = await res.json();
      found = { tag: data.tag_name, assets: (data.assets ?? []).map((a) => ({ id: a.id, name: a.name })) };
    } else {
      console.error(`release: GitHub answered ${res.status}`);
      return failed();
    }
  } catch (e) {
    console.error("release: GitHub unreachable:", e.message);
    return failed();
  }
  const stored = new Response(JSON.stringify(found), {
    headers: { "Content-Type": "application/json", "Cache-Control": `max-age=${RELEASE_TTL}` },
  });
  ctx.waitUntil(cache.put(key, stored));
  return found;
}
