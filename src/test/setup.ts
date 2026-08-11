import "@testing-library/jest-dom/vitest";

// jsdom 没有实现 matchMedia，而 antd 的响应式栅格在挂载时就会调用它。
// 缺了它，任何用到响应式布局的组件测试都会直接抛 TypeError。
if (!window.matchMedia) {
  window.matchMedia = (query: string): MediaQueryList =>
    ({
      matches: false,
      media: query,
      onchange: null,
      addListener: () => {},
      removeListener: () => {},
      addEventListener: () => {},
      removeEventListener: () => {},
      dispatchEvent: () => false,
    }) as unknown as MediaQueryList;
}
