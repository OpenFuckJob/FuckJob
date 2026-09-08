import assert from "node:assert/strict";
import test from "node:test";
import { JSDOM } from "jsdom";

import {
  markLiepinChatInput,
  runLiepinSend,
} from "../src-tauri/src/rpa/liepin/send_text.js";

function makeDocument(html) {
  return new JSDOM(html, { pretendToBeVisual: true }).window.document;
}

function makeSleepTracker() {
  const calls = [];
  const sleep = async (ms) => {
    calls.push(ms);
  };
  return { calls, sleep };
}

test("ignore hidden chat ancestors and fill without synthesizing Enter", async () => {
  const document = makeDocument(`
    <section hidden><div class="im-ui-msg-list-content"></div>
      <textarea>旧会话</textarea><button class="ant-im-btn">发送</button></section>
    <section id="active"><div class="im-ui-msg-list-content"></div>
      <textarea></textarea><button class="ant-im-btn">发送</button></section>
  `);
  let clicks = 0;
  let enterEvents = 0;
  const requestRecords = [];
  document.addEventListener("keyup", event => { if (event.key === "Enter") enterEvents++; });
  document.querySelector("section[hidden] button").onclick = () => assert.fail("hidden chat clicked");
  document.querySelector("#active button").onclick = () => {
    clicks++;
    requestRecords.push({ url: "https://api-c.liepin.com/api/com.liepin.im.c.chat.send-push", status: 200 });
  };
  const result = await runLiepinSend(document, {
    message: "你好", requestRecords, sleep: async () => {},
  });
  assert.equal(result.success, true);
  assert.equal(clicks, 1);
  assert.equal(enterEvents, 0);
  assert.equal(document.querySelector("section[hidden] textarea").value, "旧会话");
});

test("mark only the scoped chat input and clear its stale draft", () => {
  const document = makeDocument(`
    <input type="text" value="page search" />
    <div class="im-ui-msg-list-content">
      <textarea>旧草稿</textarea>
      <button class="ant-im-btn">发送</button>
    </div>
  `);

  assert.equal(markLiepinChatInput(document), true);
  assert.equal(document.querySelector("input").value, "page search");
  assert.equal(document.querySelector("textarea").value, "");
  assert.equal(
    document.querySelector("textarea").getAttribute("data-fj-liepin-chat-input"),
    "1",
  );
});

test("do not mistake chat drawer history for a priority modal", async () => {
  const document = makeDocument(`
    <div role="dialog" class="chat-drawer" data-modal="chat">
      <header>李先生 <button aria-label="Close">x</button></header>
      <div>[优先沟通] 会员权益</div>
      <div class="im-ui-msg-list-content">
        <textarea></textarea>
        <button class="ant-im-btn">发送</button>
      </div>
    </div>
  `);
  const requestRecords = [];
  const drawer = document.querySelector("[data-modal='chat']");
  let drawerClosed = false;
  drawer.querySelector("button").addEventListener("click", () => {
    drawerClosed = true;
    drawer.remove();
  });
  document.querySelector(".ant-im-btn").addEventListener("click", () => {
    requestRecords.push({
      url: "https://api-c.liepin.com/api/com.liepin.im.c.chat.send-push",
      status: 200,
    });
  });

  const result = await runLiepinSend(document, {
    message: "你好",
    requestRecords,
    sleep: async () => {},
  });

  assert.equal(drawerClosed, false);
  assert.equal(result.success, true);
});

test("close only visible priority communication modal before sending", async () => {
  const document = makeDocument(`
    <div class="ant-modal" data-modal="priority">
      <div>优先沟通</div>
      <div>体验</div>
      <button class="close-priority">x</button>
    </div>
    <div class="login-panel" data-modal="login">
      <div>登录</div>
      <button class="close-login">x</button>
    </div>
    <div class="im-ui-msg-list-content">
      <textarea></textarea>
      <button class="ant-im-btn ant-im-btn-primary">发送</button>
    </div>
  `);
  const { sleep } = makeSleepTracker();
  const requestRecords = [];
  const input = document.querySelector("textarea");
  const button = document.querySelector(".ant-im-btn");
  document.querySelector(".close-priority").addEventListener("click", () => {
    document.querySelector("[data-modal='priority']").hidden = true;
  });
  button.addEventListener("click", () => {
    input.value = "";
    requestRecords.push({
      url: "https://api-c.liepin.com/api/com.liepin.im.c.chat.send-push",
      status: 200,
    });
  });
  const result = await runLiepinSend(document, {
    message: "你好",
    requestRecords,
    sleep,
  });

  assert.equal(document.querySelector("[data-modal='priority']").hidden, true);
  assert.equal(document.querySelector("[data-modal='login']").hidden, false);
  assert.equal(result.success, true);
});

test("ignore distractor inputs and buttons outside the chat container", async () => {
  const document = makeDocument(`
    <input type="text" value="distractor" />
    <button class="send-now">send</button>
    <div class="im-ui-msg-list-content">
      <textarea></textarea>
      <button class="ant-im-btn">发送</button>
    </div>
  `);
  const { sleep } = makeSleepTracker();
  const requestRecords = [];
  const input = document.querySelector("textarea");
  const button = document.querySelector(".ant-im-btn");
  button.addEventListener("click", () => {
    input.value = "";
    requestRecords.push({
      url: "https://api-c.liepin.com/api/com.liepin.im.c.chat.send-push",
      status: 200,
    });
  });
  const result = await runLiepinSend(document, {
    message: "你好",
    requestRecords,
    sleep,
  });

  assert.equal(result.success, true);
  assert.equal(document.querySelector("input").value, "distractor");
  assert.equal(document.querySelector(".send-now").textContent, "send");
});

test("enable disabled send button after input event", async () => {
  const document = makeDocument(`
    <div class="im-ui-msg-list-content">
      <textarea></textarea>
      <button class="ant-im-btn ant-im-btn-disabled" disabled>发送</button>
    </div>
  `);
  const { sleep } = makeSleepTracker();
  const requestRecords = [];
  const input = document.querySelector("textarea");
  const button = document.querySelector(".ant-im-btn");
  input.addEventListener("input", () => {
    button.disabled = false;
    button.classList.remove("ant-im-btn-disabled");
  });
  button.addEventListener("click", () => {
    input.value = "";
    requestRecords.push({
      url: "https://api-c.liepin.com/api/com.liepin.im.c.chat.send-push",
      status: 200,
    });
  });

  const result = await runLiepinSend(document, {
    message: "你好",
    requestRecords,
    sleep,
  });

  assert.equal(result.success, true);
  assert.equal(
    document.querySelector(".ant-im-btn").disabled,
    false,
  );
});

test("retry at most once when no send-push arrives and message stays present", async () => {
  const document = makeDocument(`
    <div class="im-ui-msg-list-content">
      <textarea></textarea>
      <button class="ant-im-btn ant-im-btn-primary">发送</button>
    </div>
  `);
  const { sleep } = makeSleepTracker();
  const requestRecords = [];

  const result = await runLiepinSend(document, {
    message: "你好",
    requestRecords,
    sleep,
  });

  assert.equal(result.attempts, 2);
  assert.equal(result.retried, true);
});

test("do not retry when send-push or a new bubble is observed", async () => {
  const document = makeDocument(`
    <div class="im-ui-msg-list-content">
      <textarea></textarea>
      <button class="ant-im-btn ant-im-btn-primary">发送</button>
      <div class="bubble">你好</div>
    </div>
  `);
  const { sleep } = makeSleepTracker();
  const requestRecords = [
    { url: "https://api-c.liepin.com/api/com.liepin.im.c.chat.send-push", status: 200 },
  ];

  const result = await runLiepinSend(document, {
    message: "你好",
    requestRecords,
    sleep,
  });

  assert.equal(result.retried, false);
  assert.equal(result.attempts, 1);
});

test("leave an unknown verification modal open and return a clear error", async () => {
  const document = makeDocument(`
    <div role="dialog" data-modal="verification">
      <div>安全验证</div>
      <button class="close-verification">x</button>
    </div>
    <div class="im-ui-msg-list-content">
      <textarea></textarea>
      <button class="ant-im-btn">发送</button>
    </div>
  `);
  const result = await runLiepinSend(document, { message: "你好" });

  assert.equal(result.success, false);
  assert.match(result.reason, /^blocked-modal:/);
  assert.equal(document.querySelector("[data-modal='verification']").hidden, false);
});

test("do not treat a pre-existing identical bubble as a new send", async () => {
  const document = makeDocument(`
    <div class="im-ui-msg-list-content">
      <textarea></textarea>
      <button class="ant-im-btn">发送</button>
      <div class="bubble">你好</div>
    </div>
  `);
  const { sleep } = makeSleepTracker();
  const result = await runLiepinSend(document, {
    message: "你好",
    requestRecords: [],
    sleep,
  });

  assert.equal(result.success, false);
  assert.equal(result.attempts, 2);
  assert.equal(result.retried, true);
});

test("replace a stale draft instead of appending it to the new message", async () => {
  const document = makeDocument(`
    <div class="im-ui-msg-list-content">
      <textarea>旧草稿你好</textarea>
      <button class="ant-im-btn">发送</button>
    </div>
  `);
  const requestRecords = [];
  let submitted = "";
  document.querySelector(".ant-im-btn").addEventListener("click", () => {
    submitted = document.querySelector("textarea").value;
    requestRecords.push({
      url: "https://api-c.liepin.com/api/com.liepin.im.c.chat.send-push",
      status: 200,
    });
  });

  const result = await runLiepinSend(document, {
    message: "你好",
    requestRecords,
  });

  assert.equal(result.success, true);
  assert.equal(submitted, "你好");
});

test("keep waiting for send proof after the input clears", async () => {
  const document = makeDocument(`
    <div class="im-ui-msg-list-content">
      <textarea></textarea>
      <button class="ant-im-btn">发送</button>
    </div>
  `);
  const requestRecords = [];
  document.querySelector(".ant-im-btn").addEventListener("click", () => {
    document.querySelector("textarea").value = "";
  });
  let slept = false;
  const sleep = async () => {
    if (!slept) {
      slept = true;
      requestRecords.push({
        url: "https://api-c.liepin.com/api/com.liepin.im.c.chat.send-push",
        status: 200,
      });
    }
  };

  const result = await runLiepinSend(document, {
    message: "你好",
    requestRecords,
    sleep,
  });

  assert.equal(result.success, true);
  assert.equal(result.requestSucceeded, true);
});

test("use real timer polling when no test sleep override is provided", async () => {
  const document = makeDocument(`
    <div class="im-ui-msg-list-content">
      <textarea></textarea>
      <button class="ant-im-btn">发送</button>
    </div>
  `);
  const requestRecords = [];
  document.querySelector(".ant-im-btn").addEventListener("click", () => {
    document.defaultView.setTimeout(() => {
      requestRecords.push({
        url: "https://api-c.liepin.com/api/com.liepin.im.c.chat.send-push",
        status: 200,
      });
    }, 10);
  });

  const result = await runLiepinSend(document, {
    message: "你好",
    requestRecords,
    proofTimeoutMs: 500,
  });

  assert.equal(result.success, true);
});

test("ignore ordinary containers whose class only contains dialog text", async () => {
  const document = makeDocument(`
    <div class="dialog-preview">请登录后查看历史说明</div>
    <div class="im-ui-msg-list-content">
      <textarea></textarea>
      <button class="ant-im-btn">发送</button>
    </div>
  `);
  const requestRecords = [];
  document.querySelector(".ant-im-btn").addEventListener("click", () => {
    requestRecords.push({
      url: "https://api-c.liepin.com/api/com.liepin.im.c.chat.send-push",
      status: 200,
    });
  });

  const result = await runLiepinSend(document, {
    message: "你好",
    requestRecords,
  });

  assert.equal(result.success, true);
  assert.equal(document.querySelector(".dialog-preview").hidden, false);
});

test("wait for the priority modal mask to disappear before clicking send", async () => {
  const document = makeDocument(`
    <div class="ant-modal-root">
      <div class="ant-modal-mask"></div>
      <div class="ant-modal" data-modal="priority">
        <div>优先沟通</div>
        <div>立即体验</div>
        <button class="ant-modal-close">x</button>
      </div>
    </div>
    <div class="im-ui-msg-list-content">
      <textarea></textarea>
      <button class="ant-im-btn">发送</button>
    </div>
  `);
  const requestRecords = [];
  const modal = document.querySelector("[data-modal='priority']");
  const mask = document.querySelector(".ant-modal-mask");
  let closeClicked = false;
  document.querySelector(".ant-modal-close").addEventListener("click", () => {
    closeClicked = true;
  });
  const sleep = async () => {
    if (closeClicked) {
      modal.hidden = true;
      mask.hidden = true;
    }
  };
  let maskWasGoneAtSend = false;
  document.querySelector(".ant-im-btn").addEventListener("click", () => {
    maskWasGoneAtSend = mask.hidden;
    requestRecords.push({
      url: "https://api-c.liepin.com/api/com.liepin.im.c.chat.send-push",
      status: 200,
    });
  });

  const result = await runLiepinSend(document, {
    message: "你好",
    requestRecords,
    sleep,
  });

  assert.equal(result.success, true);
  assert.equal(maskWasGoneAtSend, true);
});
