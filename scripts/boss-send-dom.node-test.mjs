import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { JSDOM } from "jsdom";

const source = readFileSync(new URL("../src-tauri/src/rpa/boss/handler/send_message.rs", import.meta.url), "utf8");

function capture(html) {
  const match = source.match(/const BOSS_SEND_STATE_SCRIPT: &str = r#"([\s\S]*?)"#;/);
  assert.ok(match, "BOSS sending must capture the draft, button readiness and outgoing messages");
  const window = new JSDOM(html, { runScripts: "outside-only" }).window;
  return window.eval(match[1]);
}

test("capture only outgoing text messages, excluding incoming messages and cards", () => {
  const state = capture(`
    <div id="chat-input" contenteditable="true">你好</div>
    <div class="chat-op"><button class="btn-send">发送</button></div>
    <div class="message-item item-myself"><div class="text-content">旧招呼</div></div>
    <div class="message-item item-friend"><div class="text-content">你好</div></div>
    <div class="message-item item-myself"><div class="message-card-wrap">你好</div></div>
  `);
  assert.deepEqual(Array.from(state.messages), ["旧招呼"]);
  assert.equal(state.inputText, "你好");
  assert.equal(state.sendEnabled, true);
});

test("a CSS-disabled send button must not be treated as ready", () => {
  const state = capture('<div id="chat-input"></div><div class="chat-op"><button class="btn-send disabled">发送</button></div>');
  assert.equal(state.sendEnabled, false);
});

test("an unavailable composer must not be treated as a cleared draft", () => {
  const state = capture('<div class="message-item item-myself"><div class="text-content">你好</div></div>');
  assert.equal(state.inputExists, false);
  assert.equal(state.sendEnabled, false);
});

test("failed outgoing bubbles do not count as send evidence", () => {
  const state = capture(`
    <div id="chat-input"></div>
    <div class="message-item item-myself"><div class="text-content">你好</div><div class="message-status status-error"></div></div>
    <div class="message-item item-myself"><div class="text-content">已发送</div></div>
  `);
  assert.deepEqual(Array.from(state.messages), ["已发送"]);
});
