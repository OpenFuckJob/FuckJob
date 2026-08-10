import { describe, expect, it } from "vitest";
import { LLM_PRESETS, isValidLlmConfig, shouldFetchLlmModels } from "./LlmConfigPanel";

describe("LLM presets", () => {
  it("keeps supported provider endpoints and key expectations deterministic", () => {
    expect(LLM_PRESETS).toEqual({
      anthropic: { label: "Anthropic", baseUrl: "https://api.anthropic.com", requiresKey: true },
      deepseek: { label: "DeepSeek", baseUrl: "https://api.deepseek.com", requiresKey: true },
      openai: { label: "OpenAI (Completions)", baseUrl: "https://api.openai.com/v1", requiresKey: true },
      openai_responses: { label: "OpenAI (Responses)", baseUrl: "https://api.openai.com/v1", requiresKey: true },
      minimax: { label: "MiniMax", baseUrl: "https://api.minimax.io/v1", requiresKey: true },
      moonshot: { label: "Moonshot", baseUrl: "https://api.moonshot.ai/v1", requiresKey: true },
      ollama: { label: "Ollama", baseUrl: "http://127.0.0.1:11434", requiresKey: false },
      openrouter: { label: "OpenRouter", baseUrl: "https://openrouter.ai/api/v1", requiresKey: true },
      xiaomi_mimo: { label: "Xiaomi MiMo", baseUrl: "https://api.xiaomimimo.com/v1", requiresKey: true },
      zai: { label: "Z.ai", baseUrl: "https://api.z.ai/api/paas/v4", requiresKey: true },
    });
  });
});

describe("LLM config validation", () => {
  const openAiConfig = {
    provider: "openai" as const,
    base_url: "https://llm.example.test/v1",
    model: "custom-model",
  };

  it("accepts a service address and model without advanced parameters", () => {
    expect(isValidLlmConfig(openAiConfig)).toBe(true);
  });

  it("rejects blank service addresses or model names", () => {
    expect(isValidLlmConfig({ ...openAiConfig, base_url: "  " })).toBe(false);
    expect(isValidLlmConfig({ ...openAiConfig, model: "  " })).toBe(false);
    expect(isValidLlmConfig(null)).toBe(false);
  });
});

describe("LLM model list loading", () => {
  it("requires an explicit user action and saved key for key-based providers", () => {
    const config = {
      provider: "deepseek" as const,
      base_url: "https://api.deepseek.com",
      model: "",
    };

    expect(shouldFetchLlmModels(config, false, false)).toBe(false);
    expect(shouldFetchLlmModels(config, true, false)).toBe(false);
    expect(shouldFetchLlmModels(config, true, true)).toBe(true);
  });

  it("allows local Ollama model loading without a saved key", () => {
    const config = {
      provider: "ollama" as const,
      base_url: "http://127.0.0.1:11434",
      model: "",
    };

    expect(shouldFetchLlmModels(config, false, true)).toBe(true);
  });
});
