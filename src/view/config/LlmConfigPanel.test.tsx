import { describe, expect, it } from "vitest";
import { LLM_PRESETS, isValidLlmConfig } from "./LlmConfigPanel";

describe("LLM presets", () => {
  it("keeps all six provider endpoints and key expectations deterministic", () => {
    expect(LLM_PRESETS).toEqual({
      ollama: { label: "Ollama", baseUrl: "http://127.0.0.1:11434/v1", requiresKey: false },
      lm_studio: { label: "LM Studio", baseUrl: "http://127.0.0.1:1234/v1", requiresKey: false },
      openai: { label: "OpenAI", baseUrl: "https://api.openai.com/v1", requiresKey: true },
      deepseek: { label: "DeepSeek", baseUrl: "https://api.deepseek.com", requiresKey: true },
      dashscope: { label: "DashScope", baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1", requiresKey: true },
      custom: { label: "自定义", baseUrl: "", requiresKey: false },
    });
  });
});

describe("LLM config validation", () => {
  const customConfig = {
    provider: "custom" as const,
    base_url: "https://llm.example.test/v1",
    model: "custom-model",
  };

  it("accepts a service address and model without advanced parameters", () => {
    expect(isValidLlmConfig(customConfig)).toBe(true);
  });

  it("rejects blank service addresses or model names", () => {
    expect(isValidLlmConfig({ ...customConfig, base_url: "  " })).toBe(false);
    expect(isValidLlmConfig({ ...customConfig, model: "  " })).toBe(false);
    expect(isValidLlmConfig(null)).toBe(false);
  });
});
