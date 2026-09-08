const CHAT_CONTAINER_SELECTOR = ".im-ui-msg-list-content";
const INPUT_SELECTOR = "textarea, input[type='text'], [contenteditable='true']";
const SEND_BUTTON_SELECTOR = ".ant-im-btn, button.im-ui-basic-send-btn, button.ant-im-btn-primary";
const MODAL_SELECTOR = "[role='dialog'], .ant-modal";
const MODAL_MASK_SELECTOR = ".ant-modal-mask, .ant-modal-wrap";
const CHAT_INPUT_MARKER = "data-fj-liepin-chat-input";
const MODAL_CLOSE_TIMEOUT_MS = 3000;
const SEND_REQUEST_RE = /chat\.send-push|chat\.send|send-push/i;
const PRIORITY_HINT_RE = /优先沟通/;
const PRIORITY_FEATURE_RE = /(体验|购买|开通|支付|送\d+次|赠\d+次|扫码|¥|￥)/;
const BLOCKING_MODAL_RE = /(登录|验证码|验证|手机号|密码|安全验证|扫码|滑块)/;

function textOf(node) {
  return String(node?.innerText || node?.textContent || "").replace(/\s+/g, " ").trim();
}

function isVisible(node) {
  if (!node?.isConnected) return false;
  for (let current = node; current; current = current.parentElement) {
    if (current.hidden || current.getAttribute?.("aria-hidden") === "true") return false;
    const style = current.ownerDocument.defaultView.getComputedStyle(current);
    if (style.display === "none" || style.visibility === "hidden" || style.opacity === "0") return false;
  }
  return true;
}

function findVisible(list) {
  return list.find(isVisible) || null;
}

function inputValue(node) {
  if (!node) return "";
  if (node.isContentEditable) {
    return String(node.textContent || "").replace(/\s+/g, " ").trim();
  }
  return String(node.value || "");
}

function setInputValue(node, message) {
  if (!node) return false;
  const view = node.ownerDocument.defaultView;
  if (node.isContentEditable) {
    node.textContent = message;
  } else {
    const proto = node instanceof view.HTMLTextAreaElement
      ? view.HTMLTextAreaElement.prototype
      : view.HTMLInputElement.prototype;
    const descriptor = Object.getOwnPropertyDescriptor(proto, "value");
    if (descriptor?.set) {
      descriptor.set.call(node, message);
    } else {
      node.value = message;
    }
  }

  const beforeInputEvent = typeof view.InputEvent === "function"
    ? new view.InputEvent("beforeinput", { bubbles: true, cancelable: true, inputType: "insertText", data: message })
    : new view.Event("beforeinput", { bubbles: true, cancelable: true });
  const inputEvent = typeof view.InputEvent === "function"
    ? new view.InputEvent("input", { bubbles: true, cancelable: true, inputType: "insertText", data: message })
    : new view.Event("input", { bubbles: true, cancelable: true });
  node.dispatchEvent(beforeInputEvent);
  node.dispatchEvent(inputEvent);
  node.dispatchEvent(new view.Event("change", { bubbles: true }));
  return true;
}

async function closePriorityModal(modal, document, sleep) {
  const candidates = Array.from(
    modal.querySelectorAll(".ant-modal-close, button, [role='button'], a")
  ).filter((node) => isVisible(node));

  const closeButton = candidates.find((node) => {
    const text = textOf(node);
    const aria = String(node.getAttribute?.("aria-label") || "");
    return node.classList.contains("ant-modal-close")
      || /^(x|×)$/i.test(text)
      || /关闭|关闭弹窗|暂不|知道了|取消/i.test(text)
      || /关闭|dismiss|close/i.test(aria);
  });

  if (!closeButton) return false;
  const modalRoot = modal.closest(".ant-modal-root");
  closeButton.click();
  return Boolean(await waitFor(() => {
    const modalGone = !modal.isConnected || !isVisible(modal);
    const masksGone = Array.from((modalRoot || modal).querySelectorAll(MODAL_MASK_SELECTOR))
      .every((mask) => !isVisible(mask));
    return modalGone && masksGone;
  }, MODAL_CLOSE_TIMEOUT_MS, sleep));
}

function classifyModal(modal) {
  if (modal.querySelector(CHAT_CONTAINER_SELECTOR)) return "other";
  const text = textOf(modal);
  const hasPriorityTitle = Array.from(modal.querySelectorAll("h1, h2, h3, [class*='title'], [class*='header'], div, span"))
    .some((node) => textOf(node) === "优先沟通");
  if (hasPriorityTitle && PRIORITY_HINT_RE.test(text) && PRIORITY_FEATURE_RE.test(text)) {
    return "priority";
  }
  if (BLOCKING_MODAL_RE.test(text)) {
    return "blocking";
  }
  return "other";
}

async function handleKnownModal(document, sleep) {
  const visibleModals = Array.from(document.querySelectorAll(MODAL_SELECTOR)).filter(isVisible);
  const blocking = visibleModals.find((modal) => classifyModal(modal) === "blocking");
  if (blocking) {
    return {
      blocked: true,
      reason: `blocked-modal:${textOf(blocking)}`,
      closed: false,
    };
  }

  let closed = false;
  for (const modal of visibleModals) {
    if (classifyModal(modal) !== "priority") continue;
    const currentClosed = await closePriorityModal(modal, document, sleep);
    if (!currentClosed) {
      return {
        blocked: true,
        reason: "priority-modal-close-timeout",
        closed,
      };
    }
    closed = true;
  }

  return {
    blocked: false,
    reason: "",
    closed,
  };
}

function findChatContainer(document) {
  const marker = findVisible(Array.from(document.querySelectorAll(CHAT_CONTAINER_SELECTOR)));
  if (!marker) return null;
  let candidate = marker;
  for (let container = marker; container && container !== document.body; container = container.parentElement) {
    if (findChatInput(container)) {
      candidate = container;
      if (findAnySendButton(container)) return container;
    }
  }
  return candidate;
}

function findAnySendButton(container) {
  return container
    ? Array.from(container.querySelectorAll(SEND_BUTTON_SELECTOR))
      .find((node) => isVisible(node)
        && (textOf(node) === "发送" || textOf(node).includes("发送"))) || null
    : null;
}

function findChatInput(container) {
  if (!container) return null;
  const inputs = Array.from(container.querySelectorAll(INPUT_SELECTOR)).filter(isVisible);
  return inputs.find((node) => !node.disabled && !node.readOnly) || null;
}

function markLiepinChatInput(document) {
  document.querySelectorAll(`[${CHAT_INPUT_MARKER}]`)
    .forEach((node) => node.removeAttribute(CHAT_INPUT_MARKER));
  const input = findChatInput(findChatContainer(document));
  if (!input) return false;
  input.setAttribute(CHAT_INPUT_MARKER, "1");
  if (inputValue(input)) setInputValue(input, "");
  return true;
}

function isDisabled(node) {
  if (!node) return true;
  const className = String(node.getAttribute?.("class") || "");
  return Boolean(node.disabled)
    || node.getAttribute?.("aria-disabled") === "true"
    || className.includes("disabled")
    || className.includes("ant-im-btn-disabled");
}

function findSendButton(container) {
  if (!container) return null;
  const buttons = Array.from(container.querySelectorAll(SEND_BUTTON_SELECTOR)).filter(isVisible);
  return buttons.find((node) => !isDisabled(node)
    && (textOf(node) === "发送" || textOf(node).includes("发送"))) || null;
}

function matchingBubbleCount(container, message) {
  if (!container) return 0;
  return Array.from(container.querySelectorAll(".im-ui-message-item-send, .bubble, [data-message='sent']")).filter((node) => {
    if (!isVisible(node)) return false;
    return textOf(node).includes(message);
  }).length;
}

function hasMatchingBubble(container, message, initialBubbleCount) {
  return matchingBubbleCount(container, message) > initialBubbleCount;
}

function isAfter(record, since) {
  return !Number.isFinite(Number(record?.at)) || Number(record.at) > since;
}

function hasSendRequest(records, since) {
  return records.some((record) => isAfter(record, since)
    && SEND_REQUEST_RE.test(String(record?.url || "")));
}

function hasSuccessfulSendRequest(records, since) {
  return records.some((record) => {
    const url = String(record?.url || "");
    const status = Number(record?.status || 0);
    return isAfter(record, since) && SEND_REQUEST_RE.test(url)
      && status >= 200 && status < 300;
  });
}

async function waitFor(getter, timeoutMs, sleep) {
  const rounds = Math.max(1, Math.ceil(timeoutMs / 150));
  for (let round = 0; round < rounds; round += 1) {
    const value = getter();
    if (value) return value;
    await sleep(150);
  }
  return null;
}

async function observeSendState({
  requestRecords,
  container,
  input,
  message,
  sleep,
  requestSince,
  proofTimeoutMs,
  initialBubbleCount,
}) {
  let requestSeen = false;
  let requestSucceeded = false;
  let bubbleSeen = false;
  let inputCleared = false;

  const rounds = Math.max(1, Math.ceil(proofTimeoutMs / 150));
  for (let round = 0; round < rounds; round += 1) {
    requestSeen = requestSeen || hasSendRequest(requestRecords, requestSince);
    requestSucceeded = requestSucceeded || hasSuccessfulSendRequest(requestRecords, requestSince);
    bubbleSeen = bubbleSeen || hasMatchingBubble(container, message, initialBubbleCount);
    inputCleared = inputCleared || !inputValue(input).includes(message);
    if (requestSucceeded || bubbleSeen) break;
    await sleep(150);
  }

  return {
    requestSeen,
    requestSucceeded,
    bubbleSeen,
    inputCleared,
    messageStillPresent: inputValue(input).includes(message),
  };
}

async function clickSendOnce({
  container,
  input,
  message,
  requestRecords,
  requestSince,
  sleep,
  buttonTimeoutMs,
  proofTimeoutMs,
  initialBubbleCount,
}) {
  if (inputValue(input) !== message) {
    setInputValue(input, message);
  }

  const button = await waitFor(() => findSendButton(container), buttonTimeoutMs, sleep);
  if (!button) {
    return {
      clicked: false,
      requestSeen: false,
      requestSucceeded: false,
      bubbleSeen: false,
      inputCleared: false,
      messageStillPresent: inputValue(input).includes(message),
      reason: "send-button-missing",
    };
  }

  const activeButton = findSendButton(container) || button;
  if (!activeButton || isDisabled(activeButton)) {
    return {
      clicked: false,
      requestSeen: false,
      requestSucceeded: false,
      bubbleSeen: false,
      inputCleared: false,
      messageStillPresent: inputValue(input).includes(message),
      reason: "send-button-disabled",
    };
  }

  activeButton.click();
  const observed = await observeSendState({
    requestRecords,
    requestSince,
    container,
    input,
    message,
    sleep,
    proofTimeoutMs,
    initialBubbleCount,
  });
  return {
    clicked: true,
    ...observed,
    reason: observed.requestSucceeded
      ? "send-request"
      : observed.bubbleSeen
        ? "bubble"
        : observed.inputCleared
          ? "input-cleared-without-proof"
          : "no-signal",
  };
}

async function runLiepinSend(document, {
  message,
  requestRecords = null,
  sleep = null,
  requestSince = 0,
  inputTimeoutMs = 15000,
  buttonTimeoutMs = 15000,
  proofTimeoutMs = 8000,
} = {}) {
  const text = String(message || "");
  const wait = sleep || ((ms) => new Promise((resolve) => {
    document.defaultView.setTimeout(resolve, ms);
  }));
  const result = {
    success: false,
    attempts: 0,
    retried: false,
    popupClosed: false,
    blockedModal: false,
    requestSeen: false,
    requestSucceeded: false,
    bubbleSeen: false,
    inputCleared: false,
    reason: "",
  };

  if (!text.trim()) {
    return { ...result, reason: "empty-message" };
  }

  const records = requestRecords || document.defaultView?.__fjRequestRecords || [];

  const modalState = await handleKnownModal(document, wait);
  if (modalState.blocked) {
    return { ...result, blockedModal: true, reason: modalState.reason };
  }
  result.popupClosed = modalState.closed;

  let container = findChatContainer(document);
  if (!container) {
    return { ...result, reason: "chat-container-missing" };
  }

  let input = await waitFor(() => findChatInput(container), inputTimeoutMs, wait);
  if (!input) {
    return { ...result, reason: "chat-input-missing" };
  }

  let firstAttempt = await clickSendOnce({
    container,
    input,
    message: text,
    requestRecords: records,
    requestSince,
    sleep: wait,
    buttonTimeoutMs,
    proofTimeoutMs,
    initialBubbleCount: matchingBubbleCount(container, text),
  });
  result.attempts = 1;
  result.requestSeen = firstAttempt.requestSeen;
  result.requestSucceeded = firstAttempt.requestSucceeded;
  result.bubbleSeen = firstAttempt.bubbleSeen;
  result.inputCleared = firstAttempt.inputCleared;

  if (firstAttempt.requestSucceeded || firstAttempt.bubbleSeen) {
    return {
      ...result,
      success: firstAttempt.requestSucceeded || firstAttempt.bubbleSeen,
      reason: firstAttempt.reason,
    };
  }

  if (firstAttempt.requestSeen || firstAttempt.bubbleSeen || firstAttempt.inputCleared || !firstAttempt.messageStillPresent) {
    return {
      ...result,
      success: false,
      reason: firstAttempt.reason,
    };
  }

  const retryModalState = await handleKnownModal(document, wait);
  if (retryModalState.blocked) {
    return {
      ...result,
      attempts: 1,
      blockedModal: true,
      reason: retryModalState.reason,
    };
  }

  result.popupClosed = result.popupClosed || retryModalState.closed;

  container = findChatContainer(document) || container;
  input = findChatInput(container) || input;

  const secondAttempt = await clickSendOnce({
    container,
    input,
    message: text,
    requestRecords: records,
    requestSince,
    sleep: wait,
    buttonTimeoutMs,
    proofTimeoutMs,
    initialBubbleCount: matchingBubbleCount(container, text),
  });

  return {
    ...result,
    attempts: 2,
    retried: true,
    requestSeen: result.requestSeen || secondAttempt.requestSeen,
    requestSucceeded: result.requestSucceeded || secondAttempt.requestSucceeded,
    bubbleSeen: result.bubbleSeen || secondAttempt.bubbleSeen,
    inputCleared: result.inputCleared || secondAttempt.inputCleared,
    success:
      result.requestSucceeded
      || secondAttempt.requestSucceeded
      || result.bubbleSeen
      || secondAttempt.bubbleSeen,
    reason: secondAttempt.reason,
  };
}

export { markLiepinChatInput, runLiepinSend };
