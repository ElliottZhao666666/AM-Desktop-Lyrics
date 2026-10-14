const HEADERS = {
  'Access-Control-Allow-Origin': '*',
  'Cache-Control': 'no-store',
  'Content-Type': 'application/json; charset=utf-8',
};
const SEMVER = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/;

export function parseVersion(value) {
  if (typeof value !== 'string' || value.length > 128) return null;
  const match = SEMVER.exec(value.replace(/^v/, ''));
  if (!match) return null;
  const prerelease = match[4]?.split('.') ?? [];
  if (prerelease.some(part => /^\d+$/.test(part) && part.length > 1 && part.startsWith('0'))) return null;
  return { numbers: match.slice(1, 4).map(BigInt), prerelease };
}

export function compareVersions(left, right) {
  for (let i = 0; i < 3; i++) {
    if (left.numbers[i] !== right.numbers[i]) return left.numbers[i] > right.numbers[i] ? 1 : -1;
  }
  const a = left.prerelease, b = right.prerelease;
  if (!a.length || !b.length) return a.length === b.length ? 0 : a.length ? -1 : 1;
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    if (a[i] === undefined || b[i] === undefined) return a[i] === undefined ? -1 : 1;
    if (a[i] === b[i]) continue;
    const aNumeric = /^\d+$/.test(a[i]), bNumeric = /^\d+$/.test(b[i]);
    if (aNumeric && bNumeric) return BigInt(a[i]) > BigInt(b[i]) ? 1 : -1;
    if (aNumeric !== bNumeric) return aNumeric ? -1 : 1;
    return a[i] > b[i] ? 1 : -1;
  }
  return 0;
}

const json = (body, status) => new Response(JSON.stringify(body), { status, headers: HEADERS });
const noUpdate = () => new Response(null, { status: 204, headers: HEADERS });

export async function handleUpdate(url, env) {
  let parts;
  try { parts = url.pathname.split('/').slice(1).map(decodeURIComponent); }
  catch { return json({ error: 'Invalid URL encoding.' }, 400); }
  if (parts.length !== 4 || parts[0] !== 'u') return json({ error: 'Expected /u/:app_id/:platform/:current_version' }, 400);
  const [, appId, requestedPlatform, currentVersion] = parts;
  const current = parseVersion(currentVersion);
  if (!/^[a-z0-9-]{1,64}$/.test(appId) || !/^[a-z0-9_-]{1,64}$/.test(requestedPlatform) || !current) {
    return json({ error: 'Invalid app, platform or semantic version.' }, 400);
  }
  // 0.9.0 等旧版只传入 windows；当前发布的 Windows 包均为 x64。
  const platform = appId === 'amdl' && requestedPlatform === 'windows' ? 'windows-x86_64' : requestedPlatform;
  try {
    const app = await env.DB.prepare('SELECT status FROM apps WHERE app_id = ?').bind(appId).first();
    if (!app) return json({ error: 'Unknown application.' }, 404);
    if (app.status !== 'active') return noUpdate();
    const latest = await env.DB.prepare(
      'SELECT version, download_url, release_notes, pub_date, signature FROM releases WHERE app_id = ? AND platform = ? AND is_latest = 1 ORDER BY id DESC LIMIT 1',
    ).bind(appId, platform).first();
    if (!latest) return noUpdate();
    const version = parseVersion(latest.version);
    if (!version) throw Error('Invalid release version');
    if (compareVersions(version, current) <= 0) return noUpdate();
    // 正式版本用户不自动进入预览版本；预览版本用户可收到后续预览与正式版本。
    if (!current.prerelease.length && version.prerelease.length) return noUpdate();
    const signature = latest.signature?.trim();
    const download = new URL(latest.download_url);
    const date = new Date(/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/.test(latest.pub_date) ? latest.pub_date.replace(' ', 'T') + 'Z' : latest.pub_date);
    if (!signature || download.protocol !== 'https:' || !Number.isFinite(date.getTime())) {
      console.error(JSON.stringify({ event: 'invalid_update_metadata', appId, platform, version: latest.version }));
      return json({ error: 'Release metadata is incomplete.' }, 503);
    }
    return json({ version: latest.version.replace(/^v/, ''), notes: latest.release_notes ?? '', pub_date: date.toISOString(), url: download.href, signature }, 200);
  } catch (error) {
    console.error(JSON.stringify({ event: 'update_check_failed', appId, platform, message: String(error) }));
    return json({ error: 'Update service temporarily unavailable.' }, 500);
  }
}
