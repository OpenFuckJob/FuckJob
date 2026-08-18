import { fileURLToPath, URL } from "node:url";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    // 默认的 5 秒对 antd 重组件 + userEvent 的交互测试太紧，同一批测试会随机器
    // 负载时红时绿。这类抖动比没有测试更糟：一旦大家习惯了「重跑一次就好」，
    // 真正的回归也会被当成抖动放过去
    testTimeout: 15_000,
  },
});
