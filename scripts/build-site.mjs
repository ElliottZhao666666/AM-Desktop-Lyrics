/**
 * 官网公共框架的静态构建；无运行时脚本或服务器依赖。
 * 页面中加入成对的 <!-- amdl:head/header/footer --> 与 <!-- /amdl:... --> 标记，
 * 运行 pnpm site:build。--check 只检查共享区块是否已经同步。
 */
import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const docs = path.join(root, 'docs');
const checkOnly = process.argv.includes('--check');
const templates = Object.fromEntries(await Promise.all(['head', 'header', 'footer'].map(async name => [
    name, (await fs.readFile(path.join(root, 'site/partials', `${name}.html`), 'utf8')).replace(/\r\n?/g, '\n').trim(),
])));

async function* pages(directory) {
    for (const entry of await fs.readdir(directory, { withFileTypes: true })) {
        if (entry.name.startsWith('.') || entry.name === 'codex-md-drafts') continue;
        const file = path.join(directory, entry.name);
        if (entry.isDirectory()) yield* pages(file);
        else if (entry.isFile() && entry.name.endsWith('.html')) yield file;
    }
}

let count = 0;
const outdated = [];
for await (const file of pages(docs)) {
    const raw = await fs.readFile(file, 'utf8');
    const original = raw.replace(/\r\n?/g, '\n');
    if (!original.includes('<!-- amdl:')) continue;
    const prefix = path.relative(path.dirname(file), docs).split(path.sep).join('/');
    const base = prefix ? `${prefix}/` : '';
    const relative = path.relative(docs, file).split(path.sep).join('/');
    const values = {
        homeHref: `${base}index.html`,
        helpHref: `${base}help/get-token/index.html`,
        iconSrc: `${base}assets/app-icon.png`,
        cssHref: `${base}assets/site.css`,
        helpCurrent: relative.startsWith('help/') ? ' aria-current="page"' : '',
    };
    let output = original;
    for (const name of ['head', 'header', 'footer']) {
        const pattern = new RegExp(`<!-- amdl:${name} -->[\\s\\S]*?<!-- /amdl:${name} -->`, 'g');
        const matches = [...original.matchAll(pattern)];
        if (matches.length !== 1) throw new Error(`${relative}: 需要且只能有一组 amdl:${name} 标记`);
        const content = templates[name].replace(/\{\{(\w+)\}\}/g, (_, key) => {
            if (!(key in values)) throw new Error(`未知模板字段：${key}`);
            return values[key];
        });
        output = output.replace(pattern, `<!-- amdl:${name} -->\n${content}\n<!-- /amdl:${name} -->`);
    }
    if (!/\bid="main-content"/.test(output)) throw new Error(`${relative}: 正文需要 id="main-content"`);
    if (output !== original) {
        if (checkOnly) outdated.push(relative);
        else await fs.writeFile(file, output);
    } else if (!checkOnly && raw !== output) {
        await fs.writeFile(file, output);
    }
    count++;
}
if (outdated.length) {
    console.error(`公共框架尚未同步：${outdated.join('、')}。请运行 pnpm site:build。`);
    process.exitCode = 1;
} else console.log(`${checkOnly ? '已检查' : '已生成'} ${count} 个静态页面的公共框架。`);
