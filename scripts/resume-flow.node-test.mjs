import assert from 'node:assert/strict';
import test from 'node:test';
import { JSDOM } from 'jsdom';
import { readResumeUi, markResumeUi, resumeProof, resumeSelectionReady } from '../src-tauri/src/rpa/resume_ui.js';

const documentFor = html => new JSDOM(html).window.document;
const bossHtml = `<section class="chat-conversation"><header class="chat-user">吴女士</header>
  <ul class="message-list"><li class="message-item"><h3>旧简历.pdf</h3><span>点击预览附件简历</span></li></ul></section>
  <div class="dialog-wrap"><h3>请选择要发送的简历</h3><button class="dialog-close">×</button><ul>
  <li class="list-item"><div class="item-body"><span class="resume-name">测试简历.docx</span><div class="item-desc">更新于2026.10.02 06:28</div></div><div class="preview">预览</div></li>
  <li class="list-item"><div class="item-body"><span class="resume-name">AI简历.pdf</span><div class="item-desc">更新于2026.09.16 10:15</div></div><div class="preview">预览</div></li>
  </ul><button disabled>发送</button></div>`;

test('BOSS reads actual docx and pdf rows and marks only the requested preview', () => {
  const doc = documentFor(bossHtml);
  const state = readResumeUi(doc, 'boss');
  assert.deepEqual(state.candidates.map(row => row.name), ['测试简历.docx', 'AI简历.pdf']);
  assert.equal(markResumeUi(doc, 'boss', 'preview', state.candidates[0]), true);
  assert.equal(doc.querySelector('[data-fj-resume-action]').closest('li').querySelector('.resume-name').textContent, '测试简历.docx');
  assert.equal(markResumeUi(doc, 'boss', 'submit'), false);
});

test('cleanup closes preview before selection and does not touch the chat', () => {
  const doc = documentFor(bossHtml + `<div class="dialog-wrap"><h3>附件预览</h3><button class="dialog-close">×</button><iframe src="/bzl-office/pdf-viewer"></iframe></div>`);
  assert.equal(markResumeUi(doc, 'boss', 'close-preview'), true);
  const close = doc.querySelector('[data-fj-resume-action]');
  assert.equal(close.closest('.dialog-wrap').querySelector('h3').textContent, '附件预览');
  close.closest('.dialog-wrap').remove();
  assert.equal(markResumeUi(doc, 'boss', 'close'), true);
  assert.equal(doc.querySelector('[data-fj-resume-action]').closest('.dialog-wrap').querySelector('h3').textContent, '请选择要发送的简历');
  assert.equal(doc.querySelector('.chat-user').textContent, '吴女士');
});

test('historical files, attachment modal and success toast are not new chat proof', () => {
  const doc = documentFor(bossHtml);
  const before = resumeProof(doc, 'boss');
  doc.body.insertAdjacentHTML('beforeend', '<div>发送成功</div>');
  assert.deepEqual(resumeProof(doc, 'boss'), before);
  doc.querySelector('.message-list').insertAdjacentHTML('beforeend', '<li class="message-item"><h3>测试简历.docx</h3><span>点击预览附件简历</span></li>');
  assert.equal(resumeProof(doc, 'boss').cards.length, before.cards.length + 1);
});

test('Liepin identifies a single checked row without confusing its online resume', () => {
  const doc = documentFor(`<div class="ant-modal"><h3>选择附件简历</h3><p>招聘方将同时收到您的默认在线简历和附件简历</p>
    <div class="resume-row"><input type="radio" checked><span>Java简历</span><span>2026-10-02 06:28 上传</span><a>预览</a></div><button>立即投递</button></div>`);
  const state = readResumeUi(doc, 'liepin');
  assert.equal(state.kind, 'list');
  assert.deepEqual(state.candidates.map(row => row.name), ['Java简历']);
  assert.equal(markResumeUi(doc, 'liepin', 'select', state.candidates[0]), true);
  assert.equal(markResumeUi(doc, 'liepin', 'submit'), true);
});

test('dialog header is not mistaken for the attachment list and a row must be selected before submitting', () => {
  const doc=documentFor(bossHtml.replace('<h3>请选择要发送的简历</h3><button class="dialog-close">×</button>',
    '<div class="dialog-header"><h3>请选择要发送的简历</h3><button class="dialog-close">×</button></div>'));
  const candidate=readResumeUi(doc,'boss').candidates[0];
  assert.equal(candidate.name,'测试简历.docx');
  doc.querySelector('button[disabled]').disabled=false;
  assert.equal(resumeSelectionReady(doc,'boss',candidate),false);
  doc.querySelector('li.list-item').classList.add('active');
  assert.equal(resumeSelectionReady(doc,'boss',candidate),true);
  doc.querySelector('.resume-name').textContent='替换后的简历.pdf';
  assert.equal(resumeSelectionReady(doc,'boss',candidate),false);
});

test('single BOSS confirmation is recognized separately from a multiple attachment picker', () => {
  const doc=documentFor('<div class="panel-resume"><p>确定向 Boss 发送简历吗？</p><div class="btns"><span>取消</span><span>确定</span></div></div>');
  assert.equal(readResumeUi(doc,'boss').kind,'confirm');
  assert.equal(markResumeUi(doc,'boss','close'),true);
  assert.equal(doc.querySelector('[data-fj-resume-action]').textContent,'取消');
});
