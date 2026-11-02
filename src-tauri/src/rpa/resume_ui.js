const resumeText = el => (el?.textContent || '').trim();
function resumeVisible(el) {
  for (let node = el; node; node = node.parentElement) {
    const style = el.ownerDocument.defaultView.getComputedStyle(node);
    if (node.hidden || style.display === 'none' || style.visibility === 'hidden') return false;
  }
  return true;
}
function resumeEnabled(el) {
  return resumeVisible(el) && !el.disabled && el.getAttribute('aria-disabled') !== 'true'
    && !/(disabled|unable)/.test(el.className || '');
}
function resumeDialog(doc, kind) {
  const titles = kind === 'preview' ? ['附件预览'] : ['请选择要发送的简历', '选择附件简历', '确定向 Boss 发送简历吗'];
  const roots = Array.from(doc.querySelectorAll('[role=dialog], [class*=dialog], [class*=modal], .panel-resume'))
    .filter(resumeVisible).filter(el => titles.some(title => resumeText(el).replace(/\s/g,'').includes(title.replace(/\s/g,''))))
    .filter(el => kind === 'preview' ? el.querySelector('iframe')
      : el.querySelector('.resume-name, input[type=radio], [role=radio], .btns'));
  return roots.sort((a, b) => resumeText(a).length - resumeText(b).length)[0] || null;
}
function resumeRows(root, platform) {
  if (!root) return [];
  if (platform === 'boss') {
    const rows = Array.from(root.querySelectorAll('li.list-item')).filter(row => row.querySelector('.resume-name'));
    if (rows.length) return rows;
    return Array.from(root.querySelectorAll('a')).filter(el => /\.(pdf|docx?)$/i.test(resumeText(el)))
      .map(el => el.closest('li') || el.parentElement);
  }
  return Array.from(root.querySelectorAll('input[type=radio], [role=radio]')).map(radio => {
    let row = radio.parentElement;
    while (row && row !== root) {
      if (Array.from(row.querySelectorAll('a,button,span,div')).some(el => resumeText(el) === '预览')) return row;
      row = row.parentElement;
    }
    return null;
  }).filter(Boolean);
}
function resumeCandidate(row, index, platform) {
  const version = resumeText(row.querySelector('.item-desc')) || (resumeText(row).match(/\d{4}[-.]\d{2}[-.]\d{2}[^\n]*?(?:上传|\d{2}:\d{2})/) || [''])[0];
  let name = resumeText(row.querySelector('.resume-name'));
  if (!name && platform === 'boss') name = resumeText(Array.from(row.querySelectorAll('a')).find(el => /\.(pdf|docx?)$/i.test(resumeText(el))));
  if (!name && platform === 'liepin') {
    const labels = Array.from(row.querySelectorAll('span,div,p,a')).filter(el => !el.children.length)
      .map(resumeText).filter(text => text && text !== '预览' && !/^\d{4}[-.]\d{2}/.test(text));
    name = labels[0] || '';
  }
  return { index, name, version };
}
export function readResumeUi(doc, platform) {
  const dialog = resumeDialog(doc, 'selection');
  const root = dialog || (platform === 'boss' ? resumeInventory(doc) : null);
  const candidates = resumeRows(root, platform).map((row, i) => resumeCandidate(row, i, platform));
  return { kind: root ? (dialog ? (candidates.length ? 'list' : 'confirm') : 'inventory') : 'none', candidates };
}
export function markResumeUi(doc, platform, action, candidate) {
  for (const old of doc.querySelectorAll('[data-fj-resume-action]')) old.removeAttribute('data-fj-resume-action');
  let target;
  if (action === 'open') {
    target = Array.from(doc.querySelectorAll('.toolbar-btn, button, a, span, div'))
      .find(el => resumeText(el) === '发简历' && resumeEnabled(el) && !el.children.length);
    if (!target) target = Array.from(doc.querySelectorAll('.toolbar-btn, button, a'))
      .find(el => resumeText(el) === '发简历' && resumeEnabled(el));
  } else {
    const root = resumeDialog(doc, action === 'close-preview' ? 'preview' : 'selection')
      || (action === 'preview' && platform === 'boss' ? resumeInventory(doc) : null);
    if (!root) return false;
    if (action.startsWith('close')) {
      target = root.querySelector('[aria-label="Close"], [aria-label="关闭"], .dialog-close, .ant-modal-close, [class*=icon-close], [class*=close-icon]');
      if (!target) target = Array.from(root.querySelectorAll('button,a,span,div')).find(el => ['取消', '×', '✕'].includes(resumeText(el)) && !el.children.length);
    } else if (action === 'submit') {
      target = Array.from(root.querySelectorAll('button,a,.btns span,[role=button]'))
        .find(el => ['发送', '确定', '立即投递'].includes(resumeText(el)) && resumeEnabled(el));
    } else {
      const rows = resumeRows(root, platform);
      const row = rows[candidate.index];
      if (!row || JSON.stringify(resumeCandidate(row, candidate.index, platform)) !== JSON.stringify(candidate)) return false;
      if (action === 'select') target = platform === 'boss' ? row : row.querySelector('input[type=radio], [role=radio]');
      if (action === 'preview') target = Array.from(row.querySelectorAll('.preview,a,button,span,div'))
        .find(el => resumeText(el) === '预览' && resumeEnabled(el));
      if (action === 'preview' && !target && platform === 'boss') target = Array.from(row.querySelectorAll('a'))
        .find(el => resumeText(el) === candidate.name && resumeEnabled(el));
    }
  }
  if (!target || !resumeEnabled(target)) return false;
  target.setAttribute('data-fj-resume-action', '1');
  return true;
}
export function resumeProof(doc, platform) {
  const list = doc.querySelector(platform === 'boss' ? '.chat-message .im-list, .message-list' : '.im-ui-msg-list-content');
  const header = doc.querySelector('.chat-user-info, .chat-user, .chat-header, .im-ui-chat-header');
  const cards = list ? Array.from(list.querySelectorAll(platform === 'boss' ? 'li' : '.im-ui-message-item-send'))
    .filter(el => /点击预览附件简历|您的附件简历[\s\S]*已发送给Boss|在线简历[\s\S]*附件简历/.test(resumeText(el)))
    .map(el => ({ id: el.getAttribute('data-mid') || el.id, text: resumeText(el) })) : [];
  let identity = resumeText(header);
  if (!identity && list) {
    let scope = list.parentElement;
    while (scope && scope !== doc.body && !scope.querySelector('.toolbar-btn, textarea, [contenteditable]')) scope = scope.parentElement;
    if (scope && scope !== doc.body) {
      const clone = scope.cloneNode(true);
      clone.querySelectorAll('ul, .chat-message, .panel-resume, .chat-op, .toolbar-btn, textarea, [contenteditable], iframe').forEach(el => el.remove());
      identity = resumeText(clone);
    }
  }
  return { conversation: identity, cards };
}
function resumeSubmitReady(doc, platform) {
  const root = resumeDialog(doc, 'selection');
  return !!root && Array.from(root.querySelectorAll('button,a,.btns span,[role=button]'))
    .some(el => ['发送', '确定', '立即投递'].includes(resumeText(el)) && resumeEnabled(el));
}
export function resumeSelectionReady(doc, platform, candidate) {
  const root = resumeDialog(doc, 'selection');
  const row = resumeRows(root, platform)[candidate.index];
  if (!row || JSON.stringify(resumeCandidate(row,candidate.index,platform)) !== JSON.stringify(candidate)) return false;
  const radio = row.querySelector('input[type=radio], [role=radio]');
  const selected = radio ? radio.checked || radio.getAttribute('aria-checked') === 'true'
    : /(?:^|\s)(active|selected|checked)(?:\s|$)/.test(row.className) || row.getAttribute('aria-selected') === 'true';
  return selected && resumeSubmitReady(doc, platform);
}
function resumeSecurityBlocked(doc) {
  return Array.from(doc.querySelectorAll('[class*=captcha], [class*=verify], [role=dialog], .ant-modal'))
    .filter(resumeVisible).filter(el => !el.querySelector('.im-ui-msg-list-content, #chat-input'))
    .some(el => /安全验证|滑块|验证码|访问异常/.test(resumeText(el)));
}
function resumeMaskPresent(doc) {
  return Array.from(doc.querySelectorAll('.ant-modal-mask, .dialog-mask, .modal-backdrop')).some(resumeVisible);
}
function resumeInventory(doc) {
  const heading = Array.from(doc.querySelectorAll('h2,h3')).find(el => resumeText(el) === '附件管理');
  let root = heading?.parentElement;
  while (root && root !== doc.body) {
    if (resumeRows(root, 'boss').length && /文件[（(]\d+\//.test(resumeText(root))) return root;
    root = root.parentElement;
  }
  return null;
}
