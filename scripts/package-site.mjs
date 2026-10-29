/** 将 docs 中公开的网站资源复制到独立目录，防止本地草稿或旧归档进入 Pages 部署。 */
import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const docs = path.join(root, 'docs');
const builds = path.join(root, '.tauri', 'site-builds');
const allowed = new Set(['.html', '.css', '.js', '.png', '.jpg', '.jpeg', '.webp', '.svg', '.ico', '.woff2', '.xml', '.txt']);
const ignored = new Set(['codex-md-drafts']);
await fs.mkdir(builds, { recursive: true });
const output = await fs.mkdtemp(path.join(builds, 'amdl-'));
const included = [];
async function copy(directory) {
    for (const entry of await fs.readdir(directory, { withFileTypes: true })) {
        if (entry.name.startsWith('.') || ignored.has(entry.name)) continue;
        const source = path.join(directory, entry.name);
        if (entry.isSymbolicLink()) throw Error(`站点资源不允许符号链接：${source}`);
        if (entry.isDirectory()) { await copy(source); continue; }
        if (!entry.isFile() || !allowed.has(path.extname(entry.name).toLowerCase())) continue;
        const relative = path.relative(docs, source);
        const destination = path.join(output, relative);
        await fs.mkdir(path.dirname(destination), { recursive: true });
        await fs.copyFile(source, destination);
        included.push(relative.split(path.sep).join('/'));
    }
}
await copy(docs);
for (const required of ['index.html', 'help/get-token/index.html', 'help/quick-start/index.html', 'assets/app-icon.png', 'assets/download.js', 'assets/website-preview.png']) {
    if (!included.includes(required)) throw Error(`缺少必要的站点文件：${required}`);
}
for (const relative of included.filter(file => file.endsWith('.html'))) {
    const html = await fs.readFile(path.join(output, relative), 'utf8');
    for (const [, value] of html.matchAll(/(?:src|href)="([^"]+)"/g)) {
        if (/^(?:https?:|mailto:|#|data:)/i.test(value)) continue;
        const linkPath = decodeURIComponent(value.split(/[?#]/, 1)[0]);
        const target = linkPath.startsWith('/')
            ? path.resolve(output, `.${linkPath}`)
            : path.resolve(path.dirname(path.join(output, relative)), linkPath);
        if (target !== output && !target.startsWith(output + path.sep)) throw Error(`${relative}: 站内链接越过部署目录 ${value}`);
        try { await fs.access(target); } catch { throw Error(`${relative}: 站内链接目标不存在 ${value}`); }
    }
}
const manifest = path.join(root, '.tauri', 'site-refresh', 'site-manifest.json');
await fs.mkdir(path.dirname(manifest), { recursive: true });
await fs.writeFile(manifest, JSON.stringify({ output, files: included.sort() }, null, 2));
console.log(`Pages 上传目录：${output}\n公开文件：${included.length} 个；本地草稿和 assets.zip 已排除。`);
