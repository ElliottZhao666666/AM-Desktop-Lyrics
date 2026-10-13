// 可选视觉回归：先启动 Vite；Playwright 可由 AMDL_PLAYWRIGHT_MODULE 指向已有安装。
const {chromium}=require(process.env.AMDL_PLAYWRIGHT_MODULE||'playwright');
const fs=require('node:fs');
const path=require('node:path');
const assert=require('node:assert/strict');
const output=path.resolve(__dirname,'../.tauri/visual-regression');
fs.mkdirSync(output,{recursive:true});
(async()=>{
  const browser=await chromium.launch({headless:true,...(process.env.AMDL_BROWSER_EXECUTABLE?{executablePath:process.env.AMDL_BROWSER_EXECUTABLE}:{})});
  try {
    const page=await browser.newPage({viewport:{width:1180,height:760},deviceScaleFactor:1});
    const errors=[];page.on('pageerror',error=>errors.push(error.message));
    await page.goto(process.env.AMDL_KARAOKE_TEST_URL||'http://127.0.0.1:5579/scripts/fixtures/karaoke.html');
    await page.locator('.amdl-karaoke-active').first().waitFor();
    await page.evaluate(()=>document.fonts.ready);
    const settle=()=>page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
    await settle();
    const inspect=()=>page.locator('.amdl-karaoke-text').evaluateAll(elements=>elements.map(element=>{
      const normal=element.querySelector('.amdl-karaoke-glyphs:not(.is-active)'),active=element.querySelector('.is-active');
      const css=getComputedStyle(active),id=normal.style.clipPath.match(/#([^"\)]+)/)?.[1];
      return {layers:element.querySelectorAll('.amdl-karaoke-glyphs').length,normalText:normal.textContent,activeText:active.textContent,normalWidth:normal.getBoundingClientRect().width,activeWidth:active.getBoundingClientRect().width,background:css.backgroundImage,repeat:css.backgroundRepeat,extent:parseFloat(css.backgroundSize),mask:document.getElementById(id)?.querySelector('path').getAttribute('d')};
    }));
    let states=await inspect();
    for(const state of states){assert.equal(state.layers,2);assert.equal(state.normalText,state.activeText);assert.ok(state.mask.length>10);assert.equal(state.repeat,'no-repeat');assert.ok(state.extent>0);assert.ok(state.extent<=state.normalWidth+1);assert.ok(Math.abs(state.normalWidth-state.activeWidth)<1);}
    assert.match(states[0].background,/linear-gradient/);
    await page.screenshot({path:path.join(output,'gradient-partial.png'),fullPage:true});
    await page.locator('#position').evaluate(element=>{element.value='20000';element.dispatchEvent(new Event('input',{bubbles:true}));});await settle();
    const complete=await inspect();assert.ok(complete[0].extent>states[0].extent);
    await page.screenshot({path:path.join(output,'gradient-complete.png'),fullPage:true});
    await page.setViewportSize({width:700,height:820});await settle();
    const narrow=await inspect();assert.ok(narrow[2].normalWidth<states[2].normalWidth);
    await page.screenshot({path:path.join(output,'gradient-wrapped.png'),fullPage:true});
    const options=await page.locator('#preset option').evaluateAll(elements=>elements.map(element=>element.value));
    for(const option of options){await page.selectOption('#preset',option);await settle();states=await inspect();for(const state of states){assert.ok(state.mask);assert.ok(Math.abs(state.normalWidth-state.activeWidth)<1);}await page.screenshot({path:path.join(output,`theme-${option}.png`),fullPage:true});}
    await page.click('#pause');await settle();for(const state of await inspect())assert.ok(state.extent<=state.normalWidth/.95+1);
    assert.deepEqual(errors,[]);
    console.log(`通过：${options.length} 个主题、中文/英文/换行、字体变化、暂停缩放；整行只有两套字形，所有词共用渐变。`);
    console.log(`视觉结果：${output}`);
  } finally { await browser.close(); }
})().catch(error=>{console.error(error);process.exitCode=1;});
