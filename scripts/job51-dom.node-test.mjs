import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { JSDOM } from 'jsdom';
import { collectJobs51, listedJobIds51, captureMarkedJobUrl51, markQrEntry51, qrDataUrl51, applyState51, markApply51, readSelection51, markSelection51, markSliderRefresh51, sliderGeometry51, sliderPassed51, sliderStale51 } from '../src-tauri/src/rpa/job51/ui.js';
const doc = html => new JSDOM(html, {url:'https://jobs.51job.com/shenzhen/123.html'}).window.document;

test('slider stops with the handle inside the rail instead of dragging a full rail width',()=>{
  const d=doc('<div id="aliyunCaptcha-sliding-body"><div id="aliyunCaptcha-sliding-text-box"></div><span id="aliyunCaptcha-sliding-slider"></span></div>');
  const slider=d.querySelector('span');
  slider.getBoundingClientRect=()=>({x:100,y:200,width:40,height:40,right:140});
  d.querySelector('#aliyunCaptcha-sliding-body').getBoundingClientRect=()=>({x:100,y:200,width:360,height:40,right:460});
  d.querySelector('#aliyunCaptcha-sliding-text-box').getBoundingClientRect=()=>({x:140,y:200,width:320,height:40,right:460});
  assert.equal(sliderGeometry51(d).x,120);
  assert.equal(sliderGeometry51(d).end,440);
  assert.equal(sliderGeometry51(d).outerHeight,40);
  d.querySelector('#aliyunCaptcha-sliding-text-box').remove();
  assert.equal(sliderGeometry51(d).end,440);
  slider.style.display='none';
  assert.equal(sliderGeometry51(d),null);
});

test('verification requires visible job content, not an empty or login page',()=>{
  assert.equal(sliderPassed51(doc('')),false);
  assert.equal(sliderPassed51(doc('<div>页面加载中</div>')),false);
  assert.equal(sliderPassed51(doc('<div class="job_msg" style="display:none">Java开发</div>')),false);
  assert.equal(sliderPassed51(doc('<div class="job_msg">Java开发</div>')),true);
  assert.equal(sliderPassed51(doc('<div id="tips_title">访问验证</div><div class="job_msg">Java开发</div>')),false);
  assert.equal(sliderPassed51(new JSDOM('<div class="job_msg">扫码登录</div>',{url:'https://login.51job.com/login.php'}).window.document),false);
  assert.equal(sliderPassed51(new JSDOM('<div class="joblist-item">Java开发</div>',{url:'https://we.51job.com/pc/search'}).window.document),true);
  assert.equal(sliderPassed51(new JSDOM('<div>暂无职位</div>',{url:'https://we.51job.com/pc/search'}).window.document),true);
});
test('exports the official QR image nested inside a plain paragraph, ignoring hidden and unloaded images', async()=>{
  const d=doc('<img class="qrcode" src="data:image/png;base64,wrong" style="display:none"><div class="login_qr"><div class="qrImg"><p><img id="qrimg" src="data:image/png;base64,correct"></p></div></div>');
  const img=d.querySelector('#qrimg');
  img.getBoundingClientRect=()=>({width:150,height:150});
  Object.defineProperty(img,'complete',{value:true});
  Object.defineProperty(img,'naturalWidth',{get:()=>loaded?150:0});
  let loaded=false;
  assert.equal(await qrDataUrl51(d),'');
  loaded=true;
  assert.equal(await qrDataUrl51(d),'data:image/png;base64,correct');
  img.style.display='none';
  assert.equal(await qrDataUrl51(d),'');
});
test('the injected browser script exports a fetched QR image as base64 for the workspace', async()=>{
  const {window}=new JSDOM('<div class="qrImg"><p><img id="qrimg" src="/image.php?token=test"></p></div>',{url:'https://login.51job.com/login.php',runScripts:'outside-only'});
  const img=window.document.querySelector('#qrimg');
  img.getBoundingClientRect=()=>({width:150,height:150});
  Object.defineProperties(img,{complete:{value:true},naturalWidth:{value:150}});
  window.fetch=async url=>{
    assert.equal(url,'https://login.51job.com/image.php?token=test');
    return {blob:async()=>new window.Blob(['qr-image'],{type:'image/png'})};
  };
  const code=readFileSync(new URL('../src-tauri/src/rpa/job51/ui.js',import.meta.url),'utf8').replaceAll('export function ','function ').replaceAll('export async function ','async function ');
  assert.equal(await window.eval(`(() => { ${code}\nreturn qrDataUrl51(document); })()`),'data:image/png;base64,cXItaW1hZ2U=');
});
test('blocks a slider verification page even before its body loads',()=>{
  assert.equal(applyState51(doc('<title>滑动验证页面</title>'),'123').kind,'blocked');
});
test('detects verification on the search page despite its normal page title',()=>{
  const d=doc('<title>【后端,深圳招聘，求职】-前程无忧</title><div>安全验证：请按住滑块</div>');
  assert.equal(applyState51(d,'').kind,'blocked');
});
test('marks only a visible Aliyun slider refresh control',()=>{
  const d=doc('<span id="aliyunCaptcha-sliding-refresh" style="display:none">刷新</span><button>重试</button>');
  assert.equal(markSliderRefresh51(d),true);
  assert.equal(d.querySelector('[data-fj-51-slider-refresh]').textContent,'重试');
  d.querySelector('button').style.display='none';
  assert.equal(markSliderRefresh51(d),false);
});
test('treats an existing verification page as stale after thirty seconds',()=>{
  const d=doc('<div id="tips_title">访问验证</div><div id="newTraceid">请求时间: 2026-11-01 18:00:00</div>');
  assert.equal(sliderStale51(d,30_000,Date.parse('2026-11-01T18:00:29+08:00')),false);
  assert.equal(sliderStale51(d,30_000,Date.parse('2026-11-01T18:00:30+08:00')),true);
});
test('refreshes a verification page stuck loading without a request timestamp',()=>{
  const d=doc('<title>滑动验证页面</title><div>验证加载中</div>');
  Object.defineProperty(d.defaultView.performance,'now',{value:()=>30_000});
  assert.equal(sliderStale51(d),true);
});
test('marks the official QR login switch on the default phone login page',()=>{
  const d=doc(`<input type="password"><i class="passIcon" data-sensor-id="sensor_login_wechatScan"></i><button>登录 / 注册</button>`);
  assert.equal(markQrEntry51(d),true);
  assert.equal(d.querySelector('[data-fj-51-qr]').getAttribute('data-sensor-id'),'sensor_login_wechatScan');
});
test('reads main cards and excludes sidebar recommendations and chat-only cards', () => {
  const d=doc(`<div class="joblist-item"><div sensorsdata='{"jobId":"123","jobTitle":"Java开发","jobSalary":"1万","jobArea":"深圳"}'><span class="jname">Java开发</span><span class="cname">甲公司</span><button>投递</button><span>去聊聊</span></div></div><aside><a href="/456.html">推荐</a></aside>`);
  assert.equal(collectJobs51(d).length,1);
  assert.equal(collectJobs51(d)[0].platform_job_id,'123');
  assert.equal(collectJobs51(d)[0].company_name,'甲公司');
});
test('pagination sees already-applied and chat-only listings so an exhausted first page does not end scanning',()=>{
  const d=doc(`<div class="joblist-item"><div sensorsdata='{"jobId":"123"}'><span>已投递</span></div></div><div class="joblist-item"><div sensorsdata='{"jobId":"456"}'><span>去聊聊</span></div></div>`);
  assert.equal(collectJobs51(d).length,0);
  assert.deepEqual(listedJobIds51(d),['123','456']);
});
test('captures the site two-stage popup URL instead of the initial _blank placeholder',()=>{
  const d=doc('<span data-fj-51-title="1">Java开发</span>');
  const original=d.defaultView.open;
  d.querySelector('span').addEventListener('click',()=>{
    const popup=d.defaultView.open('_blank');
    popup.location.href='https://jobs.51job.com/shenzhen/12345678.html?original=1';
  });
  assert.equal(captureMarkedJobUrl51(d),'https://jobs.51job.com/shenzhen/12345678.html?original=1');
  assert.equal(d.defaultView.open,original);
});
test('apply marks the job-correlated control, never chat or hidden dialogs',()=>{
  const d=doc('<div class="apply-btn-new" id="123">立即投递</div><div>立即沟通</div><div style="display:none" class="success-popup-2">投递成功</div>');
  assert.equal(applyState51(d,'123').kind,'ready');
  assert.equal(markApply51(d,'999'),false);
  assert.equal(markApply51(d,'123'),true);
  assert.equal(d.querySelector('[data-fj-51-action]').id,'123');
});
test('success requires positive count and unknown dialogs stop automation',()=>{
  assert.equal(applyState51(doc('<div class="el-dialog__body">投递成功0个，未投递1个</div>'),'123').kind,'failed');
  assert.equal(applyState51(doc('<div class="el-dialog__body">投递成功1个，未投递0个</div>'),'123').kind,'success');
  assert.equal(applyState51(doc('<div class="el-dialog__wrapper">请选择需要投递的简历</div>'),'123').kind,'selection');
  assert.equal(applyState51(doc('<div class="apply-btn-new" id="123">已投递</div>'),'123').kind,'already');
});

test('reads online resume inventory without confusing the language selector',()=>{
  const d=doc(`<div class="apply-component-resume-dialog"><div class="pc-apply-resume__select--resume"><p title="Java简历">Java简历</p><ul style="display:none"><li title="Java简历">Java简历</li><li title="测试简历">测试简历</li></ul></div><div class="pc-apply-resume__select--lang"><p>中文</p><ul><li>中文</li><li>中英文</li></ul></div><div class="apply-component-resume-dialog-footer"><a class="btn">立即申请</a></div></div>`);
  assert.deepEqual(readSelection51(d),{kind:'online',names:['Java简历','测试简历'],selected:'Java简历'});
  assert.equal(markSelection51(d,'submit','测试简历'),false);
  assert.equal(markSelection51(d,'submit','Java简历'),true);
  assert.equal(d.querySelector('[data-fj-51-selection]').textContent,'立即申请');
});

test('attachment selection submits only a selected exact name and ignores hidden old dialogs',()=>{
  const d=doc(`<div class="apply-component-resume-dialog" style="display:none">旧弹窗</div><div class="attachment_resume_dialog"><div class="attachment_item"><div class="radio selected"></div><div class="name">Java.pdf</div></div><div class="attachment_item"><div class="radio"></div><div class="name">测试.pdf</div></div><button>发送</button></div>`);
  assert.deepEqual(readSelection51(d),{kind:'attachment',names:['Java.pdf','测试.pdf'],selected:'Java.pdf'});
  assert.equal(markSelection51(d,'submit','测试.pdf'),false);
  assert.equal(markSelection51(d,'submit','Java.pdf'),true);
  assert.equal(d.querySelector('[data-fj-51-selection]').textContent,'发送');
});
