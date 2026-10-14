/**
 * ybjun.com 统一更新服务 API
 * 部署于 Cloudflare Workers，绑定 D1 数据库
 */

// ----------------------------------------------------------------------------
// 1. 占位页 HTML 模板 (已修复 JavaScript 数组中多余的逗号)
// ----------------------------------------------------------------------------
const FALLBACK_HTML = `
<!DOCTYPE html>
<html lang="zh-CN">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Server Status: OK</title>
    <style>
        body {
            background-color: #0f7eda;
            color: white;
            font-family: "Segoe UI", "Microsoft YaHei", sans-serif;
            display: flex;
            flex-direction: column;
            align-items: flex-start;
            justify-content: center;
            height: 100vh;
            margin: 0;
            padding-left: 10%;
            padding-right: 10%;
        }
        .emoji { font-size: 100px; margin-bottom: 20px; }
        h1 { font-weight: 300; font-size: 32px; margin-bottom: 20px; line-height: 1.4; }
        p { font-size: 18px; opacity: 0.9; margin-bottom: 40px; }
        .kaomoji { font-size: 20px; margin-bottom: 50px; opacity: 0.8; font-family: "Segoe UI", "Microsoft YaHei", monospace; }
        .btn-container { display: flex; gap: 20px; flex-wrap: wrap; }
        a.btn { text-decoration: none; color: #0078D7; background-color: white; padding: 12px 25px; border-radius: 4px; font-weight: 600; font-size: 16px; transition: all 0.2s; box-shadow: 0 4px 6px rgba(0,0,0,0.1); }
        a.btn:hover { background-color: #f0f0f0; transform: translateY(-2px); box-shadow: 0 6px 12px rgba(0,0,0,0.15); }
        @media (max-width: 600px) {
            .emoji { font-size: 80px; }
            h1 { font-size: 24px; }
            body { padding-left: 20px; padding-right: 20px; }
        }
    </style>
</head>
<body>
    <div class="emoji">(・∀・)</div>
    <h1>嗨！你好像迷路到了这里。<br>放心，update.ybjun.com 连接正常！</h1>
    <p>不过这个页面空空如也（真的！）。<br>如果你是来找具体服务的，请在网址后面加上正确的 <b>端口号</b> 或 <b>子目录</b> 哦。<br>这里只是一个用来证明“我活着呢”的占位符。</p>
    <div class="kaomoji">ο(=•ω＜=)ρ⌒☆</div>
    <div class="btn-container">
        <a href="https://www.ybjun.com" class="btn" target="_self">🏠 访问主站 (ybjun.com)</a>
        <a href="javascript:void(0);" onclick="randomTeleport()" class="btn">🎲 开启时空门 (随机探索)</a>
    </div>
    <script>
        function randomTeleport() {
            const urls = [
                "https://dailywall.ybjun.com",
                "https://www.ybjun.com",
                "https://amdl.ybjun.com",
                "https://lrc.ybjun.com",
                "https://www.bing.com",
                "https://space.bilibili.com/306479391",
                "https://www.zhihu.com/people/elliot-69-70",
                "https://mp.weixin.qq.com/mp/appmsgalbum?__biz=Mzg4NDYyOTA2NA==&action=getalbum&album_id=3547814824212348930&scene=126&sessionid=1769862906388#wechat_redirect",
                "https://mp.weixin.qq.com/mp/appmsgalbum?__biz=Mzg4NDYyOTA2NA==&action=getalbum&album_id=1892429183599820801&scene=126&sessionid=1769862906388#wechat_redirect",
                "https://mp.weixin.qq.com/mp/appmsgalbum?__biz=Mzg4NDYyOTA2NA==&action=getalbum&album_id=1954109779866337283&scene=126&sessionid=1769862906388#wechat_redirect"
            ];
            const randomIndex = Math.floor(Math.random() * urls.length);
            window.location.href = urls[randomIndex];
        }
    </script>
</body>
</html>
`;

import { handleUpdate } from './updater.js';

export default {
  async fetch(request, env) {
    const headers = { 'Access-Control-Allow-Origin': '*', 'Access-Control-Allow-Methods': 'GET, HEAD, OPTIONS', 'Access-Control-Allow-Headers': 'Content-Type' };
    if (request.method === 'OPTIONS') return new Response(null, { status: 204, headers });
    if (request.method !== 'GET' && request.method !== 'HEAD') return new Response(null, { status: 405, headers: { ...headers, Allow: 'GET, HEAD, OPTIONS' } });
    const url = new URL(request.url);
    const response = url.pathname.startsWith('/u/')
      ? await handleUpdate(url, env)
      : new Response(FALLBACK_HTML, { headers: { ...headers, 'Content-Type': 'text/html; charset=utf-8' } });
    return request.method === 'HEAD' ? new Response(null, { status: response.status, headers: response.headers }) : response;
  },
};