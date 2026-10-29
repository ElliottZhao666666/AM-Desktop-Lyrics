(() => {
    const endpoint = 'https://update.ybjun.com/api/releases/amdl/windows-x86_64/latest';
    const fallback = 'https://github.com/ElliottZhao666666/AM-Desktop-Lyrics/releases/latest';
    const all = selector => document.querySelectorAll(selector);
    const set = (selector, value) => all(selector).forEach(element => { element.textContent = value; });
    const secureUrl = value => {
        if (typeof value !== 'string') return null;
        try { const url = new URL(value); return url.protocol === 'https:' ? url.href : null; }
        catch { return null; }
    };
    const setStatus = message => all('[data-release-status]').forEach(element => {
        const link = element.querySelector('[data-release-notes]');
        element.replaceChildren(document.createTextNode(message + ' '));
        if (link) element.append(link);
    });
    const load = async () => {
        try {
            const controller = new AbortController();
            const timer = setTimeout(() => controller.abort(), 8000);
            let response;
            try { response = await fetch(endpoint, { signal: controller.signal, credentials: 'omit', mode: 'cors' }); }
            finally { clearTimeout(timer); }
            if (!response.ok) throw new Error('Release endpoint unavailable');
            const release = await response.json();
            const download = secureUrl(release.download_url);
            if (release.app_id !== 'amdl' || release.platform !== 'windows-x86_64' ||
                typeof release.version !== 'string' || !/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(release.version) || !download) throw new Error('Invalid release metadata');
            const notes = secureUrl(release.release_url) || fallback;
            const versionText = `v${release.version}${release.channel === 'preview' ? ' · 预览版' : ''}`;
            set('[data-release-version]', versionText);
            set('[data-release-size]', Number.isSafeInteger(release.size_bytes) && release.size_bytes > 0 ? `${(release.size_bytes / 1048576).toFixed(2)} MB` : '大小见 Release');
            const date = new Date(release.published_at);
            if (Number.isFinite(date.getTime())) set('[data-release-date]', new Intl.DateTimeFormat('zh-CN', { timeZone: 'Asia/Shanghai', year: 'numeric', month: 'long', day: 'numeric' }).format(date) + '发布');
            all('[data-download]').forEach(element => { element.href = download; element.setAttribute('aria-label', `下载 AMDL ${versionText} Windows x64 安装包`); });
            all('[data-release-notes]').forEach(element => { element.href = notes; });
            if (/^[a-fA-F0-9]{64}$/.test(release.sha256 || '')) {
                set('[data-release-sha]', release.sha256.toLowerCase());
                all('[data-release-sha-wrap]').forEach(element => { element.hidden = false; });
            }
            setStatus('版本与安装包信息已从更新服务同步。');
            const schema = document.getElementById('software-data');
            if (schema) {
                try { const data = JSON.parse(schema.textContent); data.softwareVersion = release.version; data.downloadUrl = download; schema.textContent = JSON.stringify(data); }
                catch { /* 页面链接已更新，结构化数据损坏不应影响下载。 */ }
            }
        } catch {
            setStatus('暂时无法读取版本信息，请在此查看最新发布：');
        }
    };
    load();
})();
