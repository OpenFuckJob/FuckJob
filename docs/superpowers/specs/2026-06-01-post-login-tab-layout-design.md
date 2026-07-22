# 登录后主布局优化设计

## 背景

登录成功后，应用需要从单一账户页面调整为带顶部导航的主应用布局。用户希望顶部左侧展示 logo 和 title，顶部使用 tab 展示五个功能入口：工作台、沟通调试、简历优化、配置中心、我的；下方展示当前 tab 对应的页面内容。

## 目标

- 已登录状态展示统一的应用壳布局。
- 顶部左侧展示品牌标识和标题。
- 顶部 tab 包含：
  - 工作台：`workspace`
  - 沟通调试：`conversation-debug`
  - 简历优化：`resume-optimizer`
  - 配置中心：`config`
  - 我的：`account`
- 点击 tab 只在当前窗口内切换内容，不同步 URL 路由。
- 保留“我的”页面现有账户资料、积分、改密和退出登录能力。

## 非目标

- 不引入 React Router 或深链能力。
- 不改变登录认证流程。
- 不扩展各业务页面的实际功能内容。

## 方案

在 `App.tsx` 中保留现有登录态检查逻辑。未登录时继续渲染 `AuthPage`；已登录时渲染新的应用壳布局。应用壳内部维护 `activeTab` state，默认值为 `workspace`，通过 Ant Design `Tabs` 切换当前 tab。

内容区根据 `activeTab` 渲染对应页面组件：`WorkspacePage`、`ConversationDebugPage`、`ResumeOptimizerPage`、`ConfigPage`、`AccountPage`。`AccountPage` 继续接收 `onLoggedOut`，退出登录后由 `App.tsx` 清空 session 并返回登录页。

`AccountPage` 需要从整页布局调整为内容组件：移除它自己的全屏外层和顶部标题区域，保留账号信息、积分和重置密码卡片，避免和全局应用头部重复。

## 视觉结构

整体结构为：

1. 页面背景：浅灰背景。
2. 顶部栏：白底圆角卡片，左侧 logo + title，右侧 tab 导航。
3. 内容区：白底圆角容器，展示当前 tab 页面内容。

窄屏时顶部栏允许换行，logo/title 与 tab 可上下排列，保证 tab 可点击和内容不溢出。

## 验证

- 运行 TypeScript 或项目构建，确保类型和编译通过。
- 启动前端后验证：
  - 登录后默认展示工作台。
  - 五个 tab 可在当前窗口内切换。
  - “我的”页面能正常展示账户内容。
  - “我的”页面退出登录后返回登录页。
