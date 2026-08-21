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

// 同理，jsdom 没有 ResizeObserver。antd 的 Tabs / Segmented 这类会测量自身尺寸的
// 组件挂载时就要用它，缺了它整棵树都渲染不出来。
if (!window.ResizeObserver) {
  window.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
}

// jsdom 的元素没有 scrollIntoView。聊天类界面挂载后就会滚到底部，
// 让它成为空操作，比在组件里到处写「测试环境跳过」干净得多。
if (!Element.prototype.scrollIntoView) {
  Element.prototype.scrollIntoView = () => {};
}
