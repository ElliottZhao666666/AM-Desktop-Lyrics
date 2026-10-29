import { parseVersion } from './updater.js';

const headers = { 'Access-Control-Allow-Origin': '*', 'Content-Type': 'application/json; charset=utf-8', 'Cache-Control': 'no-store' };
const json = (body, status, cache = 'no-store') => new Response(JSON.stringify(body), { status, headers: { ...headers, 'Cache-Control': cache } });

export async function handleReleaseInfo(url, env) {
  let parts;
  try { parts = url.pathname.split('/').slice(1).map(decodeURIComponent); }
  catch { return json({ error: 'Invalid URL encoding.' }, 400); }
  if (parts.length !== 5 || parts[0] !== 'api' || parts[1] !== 'releases' || parts[4] !== 'latest') return json({ error: 'Unknown release endpoint.' }, 404);
  const [, , appId, platform] = parts;
  if (!/^[a-z0-9-]{1,64}$/.test(appId) || !/^[a-z0-9_-]{1,64}$/.test(platform)) return json({ error: 'Invalid app or platform.' }, 400);
  try {
    const release = await env.DB.prepare(
      'SELECT a.app_name, r.version, r.platform, r.download_url, r.pub_date, r.signature, r.size_bytes, r.sha256, r.release_url FROM releases r JOIN apps a ON a.app_id = r.app_id WHERE r.app_id = ? AND r.platform = ? AND r.is_latest = 1 AND a.status = ? ORDER BY r.id DESC LIMIT 1',
    ).bind(appId, platform, 'active').first();
    if (!release) return json({ error: 'No published release.' }, 404);
    const version = parseVersion(release.version);
    const download = new URL(release.download_url);
    const page = release.release_url ? new URL(release.release_url) : null;
    const date = new Date(/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/.test(release.pub_date) ? release.pub_date.replace(' ', 'T') + 'Z' : release.pub_date);
    if (!version || !release.signature?.trim() || download.protocol !== 'https:' || (page && page.protocol !== 'https:') || !Number.isFinite(date.getTime())) throw Error('Incomplete release metadata');
    return json({
      app_id: appId,
      name: release.app_name,
      version: release.version.replace(/^v/, ''),
      channel: version.prerelease.length ? 'preview' : 'stable',
      platform: release.platform,
      download_url: download.href,
      release_url: page?.href ?? null,
      size_bytes: Number.isSafeInteger(release.size_bytes) && release.size_bytes > 0 ? release.size_bytes : null,
      sha256: /^[a-fA-F0-9]{64}$/.test(release.sha256 ?? '') ? release.sha256.toLowerCase() : null,
      published_at: date.toISOString(),
    }, 200, 'public, max-age=60, stale-while-revalidate=300');
  } catch (error) {
    console.error(JSON.stringify({ event: 'release_info_failed', appId, platform, message: String(error) }));
    return json({ error: 'Release information temporarily unavailable.' }, 503);
  }
}
