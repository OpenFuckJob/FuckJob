function visible51(el) {
  if (!el) return false;
  for (let node=el; node && node.nodeType===1; node=node.parentElement) {
    const style=node.ownerDocument.defaultView.getComputedStyle(node);
    if (node.hidden || style.display==='none' || style.visibility==='hidden') return false;
  }
  return true;
}
function text51(el) { return (el?.innerText || el?.textContent || '').trim(); }
export async function qrDataUrl51(doc) {
  const candidates=[...doc.querySelectorAll('#qrimg,.qrImg img,.login_qr canvas'),...doc.querySelectorAll('img,canvas')];
  const el=candidates.find(e=>visible51(e) && (e.matches('#qrimg,.qrImg img,.login_qr canvas') || /qr|code/i.test(e.className+' '+(e.src||'')+' '+(e.parentElement?.className||''))) && e.getBoundingClientRect().width>100 && (e.tagName==='CANVAS' || (e.complete && e.naturalWidth>0)));
  if (!el) return '';
  if (el.tagName==='CANVAS') return el.toDataURL('image/png');
  if (el.src.startsWith('data:')) return el.src;
  const blob=await (await doc.defaultView.fetch(el.src)).blob();
  return await new Promise((resolve,reject)=>{
    const reader=new doc.defaultView.FileReader();
    reader.onload=()=>resolve(reader.result);
    reader.onerror=()=>reject(new Error('51job QR image export failed'));
    reader.readAsDataURL(blob);
  });
}
export function markQrEntry51(doc) {
  const el=Array.from(doc.querySelectorAll('[data-sensor-id="sensor_login_wechatScan"]')).find(visible51);
  if (!el) return false;
  el.setAttribute('data-fj-51-qr','1'); return true;
}
export function listedJobIds51(doc) {
  return Array.from(doc.querySelectorAll('.joblist-item [sensorsdata]')).flatMap(el=>{
    try { const id=String(JSON.parse(el.getAttribute('sensorsdata')||'{}').jobId||''); return /^\d+$/.test(id)?[id]:[]; }
    catch { return []; }
  });
}
export function collectJobs51(doc) {
  return Array.from(doc.querySelectorAll('.joblist-item')).flatMap(card=>{
    let data;
    try { data=JSON.parse(card.querySelector('[sensorsdata]')?.getAttribute('sensorsdata') || '{}'); } catch { return []; }
    const id=String(data.jobId || '');
    if (!/^\d+$/.test(id)) return [];
    const hasApply=Array.from(card.querySelectorAll('button,.apply-btn-new')).some(el=>visible51(el) && /^(立即)?投递$/.test(text51(el)) && !el.disabled);
    if (!hasApply) return [];
    const link=Array.from(card.querySelectorAll('a[href]')).find(a=>/\/\d+\.html|[?&]jobId=\d+/.test(a.href));
    return [{platform:'51job', platform_job_id:id, title:data.jobTitle || text51(card.querySelector('.jname')),
      company_name:text51(card.querySelector('.cname')), detail:'', salary:data.jobSalary || text51(card.querySelector('.sal')),
      location:data.jobArea || text51(card.querySelector('.area')) || null, recruiter_active_time:null, detail_url:link?.href || ''}];
  });
}
export function markTitle51(doc, id) {
  doc.querySelectorAll('[data-fj-51-title]').forEach(e=>e.removeAttribute('data-fj-51-title'));
  const card=Array.from(doc.querySelectorAll('.joblist-item')).find(e=>{
    try {return String(JSON.parse(e.querySelector('[sensorsdata]')?.getAttribute('sensorsdata')||'{}').jobId)===id;} catch {return false;}
  });
  const title=card?.querySelector('.jname');
  if (!title) return false;
  title.setAttribute('data-fj-51-title','1'); return true;
}
export function captureMarkedJobUrl51(doc) {
  const view=doc.defaultView;
  const original=view.open;
  let url='';
  const capture=value=>{ if(value && value!=='_blank' && value!=='about:blank') url=String(value); };
  view.open=function(value) {
    capture(value);
    const location={assign:capture,replace:capture};
    Object.defineProperty(location,'href',{set:capture,get:()=>url});
    return {location,focus(){},close(){}};
  };
  try { doc.querySelector('[data-fj-51-title="1"]')?.click(); return url; }
  finally { view.open=original; }
}
export function applyState51(doc, id) {
  const body=text51(doc.body);
  const tips=text51(doc.getElementById('tips_title'));
  if (doc.getElementById('tips_title') || /滑动验证|访问验证|请按住滑块|安全验证|人机验证/.test(doc.title+' '+body+' '+tips)) {
    if (/验证超时|请点击刷新|请点击重试|请刷新页面/.test(body)) return {kind:'blocked_timeout'};
    return {kind:'blocked'};
  }
  if (doc.location.hostname==='login.51job.com' || /扫码登录|验证码登录/.test(body)) return {kind:'login'};
  const notices=Array.from(doc.querySelectorAll('.el-dialog__wrapper,.el-dialog__body,.el-message,.successContent,.success-popup-2,[role="alert"]')).filter(visible51);
  const notice=notices.map(text51).join('\n');
  if (/投递.*上限|申请.*上限|操作频繁/.test(notice)) return {kind:'limit'};
  if (/投递成功\s*0\s*个|投递失败|申请失败/.test(notice)) return {kind:'failed'};
  if (/投递成功|申请成功/.test(notice)) return {kind:'success'};
  const button=Array.from(doc.querySelectorAll('.apply-btn-new')).find(e=>e.id===id && visible51(e));
  if (button && /已投递|已申请/.test(text51(button))) return {kind:'already'};
  if (notices.some(e=>/请选择需要投递的简历|选择需要同步发送的附件简历/.test(text51(e)))) return {kind:'selection'};
  if (notices.length) return {kind:'dialog', message:notice.slice(0,240)};
  if (button && /^(立即)?投递$/.test(text51(button)) && !button.disabled && !/disabled/.test(button.className)) return {kind:'ready'};
  return {kind:'unknown'};
}
export function markApply51(doc,id) {
  doc.querySelectorAll('[data-fj-51-action]').forEach(e=>e.removeAttribute('data-fj-51-action'));
  if (applyState51(doc,id).kind!=='ready') return false;
  const el=Array.from(doc.querySelectorAll('.apply-btn-new')).find(e=>e.id===id && visible51(e));
  el.setAttribute('data-fj-51-action','1'); return true;
}

function selectionDialog51(doc) {
  const attachment=Array.from(doc.querySelectorAll('.attachment_resume_dialog')).find(visible51);
  if (attachment) return {kind:'attachment',dialog:attachment};
  const online=Array.from(doc.querySelectorAll('.apply-component-resume-dialog')).find(visible51);
  if (online) return {kind:'online',dialog:online};
  return null;
}
export function readSelection51(doc) {
  const active=selectionDialog51(doc);
  if (!active) return {kind:'none',names:[],selected:''};
  if (active.kind==='online') {
    const select=active.dialog.querySelector('.pc-apply-resume__select--resume');
    return {kind:'online', names:Array.from(select?.querySelectorAll('li[title]') || []).map(e=>e.getAttribute('title')),
      selected:select?.querySelector('p[title]')?.getAttribute('title') || ''};
  }
  const rows=Array.from(active.dialog.querySelectorAll('.attachment_item'));
  return {kind:'attachment', names:rows.map(e=>text51(e.querySelector('.name'))),
    selected:text51(rows.find(e=>e.querySelector('.radio.selected'))?.querySelector('.name'))};
}
export function markSelection51(doc, action, name) {
  doc.querySelectorAll('[data-fj-51-selection]').forEach(e=>e.removeAttribute('data-fj-51-selection'));
  const active=selectionDialog51(doc);
  if (!active) return false;
  const state=readSelection51(doc);
  if (!state.names.includes(name)) return false;
  let el;
  if (action==='submit') {
    if (state.selected!==name) return false;
    el=Array.from(active.dialog.querySelectorAll('button,a.btn')).find(e=>visible51(e) && !e.disabled &&
      text51(e)===(active.kind==='online'?'立即申请':'发送'));
  } else if (action==='open' && active.kind==='online') {
    el=active.dialog.querySelector('.pc-apply-resume__select--resume > p');
  } else if (action==='select') {
    el=active.kind==='online' ? Array.from(active.dialog.querySelectorAll('.pc-apply-resume__select--resume li[title]')).find(e=>e.getAttribute('title')===name && visible51(e))
      : Array.from(active.dialog.querySelectorAll('.attachment_item')).find(e=>text51(e.querySelector('.name'))===name);
  }
  if (!el || !visible51(el)) return false;
  el.setAttribute('data-fj-51-selection','1'); return true;
}

export function sliderPassed51(doc) {
  if (!['jobs.51job.com','we.51job.com'].includes(doc.location.hostname)) return false;
  const state=applyState51(doc,'').kind;
  if (['blocked','blocked_timeout','login','limit'].includes(state)) return false;
  return ['success','selection'].includes(state)
    || Array.from(doc.querySelectorAll('.job_msg,.joblist-item')).some(el=>visible51(el) && text51(el).length>0)
    || (doc.location.hostname==='we.51job.com' && /暂无职位|没有找到/.test(text51(doc.body)));
}

export function sliderGeometry51(doc) {
  const s = doc.getElementById('aliyunCaptcha-sliding-slider')
    || doc.querySelector('.nc_iconfont.btn_slide, .btn_slide, [id*="sliding-slider"]');
  const b = doc.getElementById('aliyunCaptcha-sliding-body')
    || doc.getElementById('aliyunCaptcha-sliding-text-box')
    || doc.querySelector('.nc_scale, [id*="sliding-body"]');
  if (!visible51(s) || !visible51(b)) return null;
  const r = s.getBoundingClientRect();
  const q = b.getBoundingClientRect();
  const end = q.right - r.width / 2;
  if (r.width <= 0 || r.height <= 0 || q.width <= r.width || end <= r.x + r.width / 2) return null;
  return {
    x: r.x + r.width / 2,
    y: r.y + r.height / 2,
    end,
    width: r.width,
    outerWidth: q.width,
    outerHeight: q.height
  };
}

export function isSliderTimeout51(doc) {
  const body = text51(doc.body);
  const title = text51(doc.getElementById('tips_title'));
  return /验证超时|请点击刷新|请点击重试|请刷新页面|验证失败|网络不给力/.test(doc.title + ' ' + body + ' ' + title);
}

export function markSliderRefresh51(doc) {
  doc.querySelectorAll('[data-fj-51-slider-refresh]').forEach(el=>el.removeAttribute('data-fj-51-slider-refresh'));
  const candidates=[doc.getElementById('aliyunCaptcha-sliding-refresh'),
    ...doc.querySelectorAll('button,a,span,[role="button"]')];
  const el=candidates.find(el=>visible51(el) && (el?.id==='aliyunCaptcha-sliding-refresh' || /刷新|重试/.test(text51(el))));
  if (!el) return false;
  el.setAttribute('data-fj-51-slider-refresh','1');
  return true;
}

export function sliderStale51(doc, maxAgeMs=30000, now=Date.now()) {
  if (!['jobs.51job.com','we.51job.com'].includes(doc.location.hostname)) return false;
  if (!['blocked','blocked_timeout'].includes(applyState51(doc,'').kind)) return false;
  const match=text51(doc.getElementById('newTraceid')).match(/(?:请求时间|TIME)\s*[:：]\s*(\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2})/i);
  if (!match) return (doc.defaultView?.performance?.now?.() ?? 0)>=maxAgeMs;
  const requested=Date.parse(match[1].replace(' ','T')+'+08:00');
  return Number.isFinite(requested) && now>=requested && now-requested>=maxAgeMs;
}
