# Post Login Tab Layout Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a logged-in application shell with logo/title, top tabs, and in-window content switching for the five post-login pages.

**Architecture:** Keep authentication state in `App.tsx`. When logged in, render a local app shell that uses React state and Ant Design `Tabs` to switch page content without URL routing. Convert `AccountPage` from a full-page wrapper into an embeddable content page while preserving logout, profile, points, and password features.

**Tech Stack:** React 19, TypeScript, Ant Design 6, Vite, existing Tauri auth bridge.

---

## File Structure

- Modify `src/App.tsx`: import all post-login pages, define tab metadata, add `activeTab` state, render app shell when session exists.
- Modify `src/App.css`: add layout classes for the app shell, header, brand area, tabs, content container, and responsive behavior; adjust account classes now that account is embedded.
- Modify `src/view/account/index.tsx`: remove full-page account wrapper and duplicate topbar title while keeping `contextHolder`, account cards, and logout button.
- No new files are required.

---

### Task 1: Add Logged-In App Shell

**Files:**
- Modify: `/Users/pgthinker/IdeaProjects/fuckJob/fuck_job/src/App.tsx`

- [ ] **Step 1: Replace `App.tsx` with the logged-in shell implementation**

```tsx
import { useEffect, useState } from "react";
import { Spin, Tabs, Typography } from "antd";
import "./App.css";
import AccountPage from "./view/account";
import AuthPage from "./view/auth";
import ConfigPage from "./view/config";
import ConversationDebugPage from "./view/conversation-debug";
import ResumeOptimizerPage from "./view/resume-optimizer";
import WorkspacePage from "./view/workspace";
import { currentSession } from "./lib/tauriAuth";
import type { AuthSession } from "./types/auth";

type AppTabKey = "workspace" | "conversation-debug" | "resume-optimizer" | "config" | "account";

const appTabs: Array<{ key: AppTabKey; label: string }> = [
  { key: "workspace", label: "工作台" },
  { key: "conversation-debug", label: "沟通调试" },
  { key: "resume-optimizer", label: "简历优化" },
  { key: "config", label: "配置中心" },
  { key: "account", label: "我的" },
];

interface LoggedInAppProps {
  onLoggedOut: () => void;
}

function renderTabContent(activeTab: AppTabKey, onLoggedOut: () => void) {
  switch (activeTab) {
    case "workspace":
      return <WorkspacePage />;
    case "conversation-debug":
      return <ConversationDebugPage />;
    case "resume-optimizer":
      return <ResumeOptimizerPage />;
    case "config":
      return <ConfigPage />;
    case "account":
      return <AccountPage onLoggedOut={onLoggedOut} />;
  }
}

function LoggedInApp({ onLoggedOut }: LoggedInAppProps) {
  const [activeTab, setActiveTab] = useState<AppTabKey>("workspace");

  return (
    <main className="app-shell">
      <header className="app-header">
        <div className="app-brand">
          <div className="app-logo">FJ</div>
          <div>
            <Typography.Title level={4} className="app-title">
              Fuck Job
            </Typography.Title>
            <Typography.Text type="secondary">AI 求职工作台</Typography.Text>
          </div>
        </div>
        <Tabs
          activeKey={activeTab}
          className="app-tabs"
          items={appTabs}
          onChange={(key) => setActiveTab(key as AppTabKey)}
        />
      </header>

      <section className="app-content">{renderTabContent(activeTab, onLoggedOut)}</section>
    </main>
  );
}

function App() {
  const [session, setSession] = useState<AuthSession | null>(null);
  const [checkingSession, setCheckingSession] = useState(true);

  useEffect(() => {
    let mounted = true;

    currentSession()
      .then((value) => {
        if (mounted) {
          setSession(value);
        }
      })
      .catch(() => {
        if (mounted) {
          setSession(null);
        }
      })
      .finally(() => {
        if (mounted) {
          setCheckingSession(false);
        }
      });

    return () => {
      mounted = false;
    };
  }, []);

  if (checkingSession) {
    return (
      <main className="app-loading">
        <Spin size="large" tip="正在检查登录状态" />
      </main>
    );
  }

  return session ? (
    <LoggedInApp onLoggedOut={() => setSession(null)} />
  ) : (
    <AuthPage onLoggedIn={setSession} />
  );
}

export default App;
```

- [ ] **Step 2: Run TypeScript/build check**

Run:

```bash
npm run build
```

Expected: build may fail because CSS and account embedding have not been adjusted yet, but there should be no TypeScript errors from `App.tsx` imports or `AppTabKey` usage.

---

### Task 2: Embed Account Page Content

**Files:**
- Modify: `/Users/pgthinker/IdeaProjects/fuckJob/fuck_job/src/view/account/index.tsx`

- [ ] **Step 1: Replace the returned JSX in `AccountPage`**

Replace the existing `return (...)` block in `AccountPage` with:

```tsx
  return (
    <div className="account-page">
      {contextHolder}
      <Space direction="vertical" size="large" className="account-container">
        <div className="account-actions">
          <Button onClick={() => void submitLogout()}>退出登录</Button>
        </div>

        <Card>
          <Skeleton loading={profileLoading} active avatar paragraph={{ rows: 4 }}>
            <div className="profile-summary">
              <Avatar size={72} src={avatarUrl}>{avatarText}</Avatar>
              <div>
                <Typography.Title level={3}>{displayName}</Typography.Title>
                <Typography.Text type="secondary">{user?.email || "未设置邮箱"}</Typography.Text>
              </div>
            </div>
            <Descriptions column={2} bordered className="account-descriptions">
              <Descriptions.Item label="用户名">{user?.userName || "-"}</Descriptions.Item>
              <Descriptions.Item label="昵称">{user?.nickName || "-"}</Descriptions.Item>
              <Descriptions.Item label="邮箱">{user?.email || "-"}</Descriptions.Item>
              <Descriptions.Item label="用户 ID">{user?.userId || "-"}</Descriptions.Item>
              <Descriptions.Item label="租户 ID">{user?.tenantId || "-"}</Descriptions.Item>
              <Descriptions.Item label="部门">{user?.deptName || "-"}</Descriptions.Item>
              <Descriptions.Item label="角色组">{profile?.roleGroup || "-"}</Descriptions.Item>
              <Descriptions.Item label="岗位组">{profile?.postGroup || "-"}</Descriptions.Item>
            </Descriptions>
          </Skeleton>
        </Card>

        <Card>
          <Skeleton loading={pointsLoading} active paragraph={{ rows: 1 }}>
            <Statistic title="积分额度" value={points?.points ?? 0} suffix="点" />
            {!points && <Typography.Text type="secondary">暂无积分数据或加载失败</Typography.Text>}
          </Skeleton>
        </Card>

        <Card title="重置密码">
          <Form form={form} layout="vertical" onFinish={submitPassword} autoComplete="off">
            <Form.Item name="oldPassword" label="旧密码" rules={[{ required: true, message: "请输入旧密码" }]}>
              <Input.Password placeholder="请输入旧密码" />
            </Form.Item>
            <Form.Item name="newPassword" label="新密码" rules={[{ required: true, message: "请输入新密码" }, { min: 5, max: 30, message: "密码长度为 5-30 位" }]}>
              <Input.Password placeholder="请输入新密码" />
            </Form.Item>
            <Form.Item name="confirmPassword" label="确认新密码" dependencies={["newPassword"]} rules={[{ required: true, message: "请再次输入新密码" }, ({ getFieldValue }) => ({ validator(_, value) { return !value || getFieldValue("newPassword") === value ? Promise.resolve() : Promise.reject(new Error("两次密码不一致")); } })]}>
              <Input.Password placeholder="请再次输入新密码" />
            </Form.Item>
            <Button type="primary" htmlType="submit" loading={passwordLoading}>
              保存新密码
            </Button>
          </Form>
        </Card>
      </Space>
    </div>
  );
```

- [ ] **Step 2: Run TypeScript/build check**

Run:

```bash
npm run build
```

Expected: TypeScript compiles. Visual spacing may still need CSS in the next task.

---

### Task 3: Add Shell and Responsive Styles

**Files:**
- Modify: `/Users/pgthinker/IdeaProjects/fuckJob/fuck_job/src/App.css`

- [ ] **Step 1: Replace `App.css` with the updated stylesheet**

```css
:root {
  font-family: Inter, Avenir, Helvetica, Arial, sans-serif;
  color: #172033;
  background: #f5f7fb;
  font-synthesis: none;
  text-rendering: optimizeLegibility;
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
  -webkit-text-size-adjust: 100%;
}

body {
  margin: 0;
  min-width: 360px;
  min-height: 100vh;
}

button,
input {
  font-family: inherit;
}

.full-width {
  width: 100%;
}

.app-loading,
.auth-page,
.app-shell {
  min-height: 100vh;
}

.app-loading,
.auth-page {
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 32px;
  box-sizing: border-box;
}

.auth-page {
  background: radial-gradient(circle at top left, #dbeafe 0, transparent 32%), #f5f7fb;
}

.auth-card {
  width: min(460px, 100%);
  border-radius: 20px;
  box-shadow: 0 20px 60px rgba(30, 41, 59, 0.12);
}

.auth-header {
  text-align: center;
}

.auth-header h2,
.profile-summary h3,
.app-title {
  margin: 0;
}

.captcha-image {
  display: block;
  width: 96px;
  height: 30px;
  object-fit: contain;
}

.app-shell {
  padding: 24px;
  box-sizing: border-box;
  background: #f5f7fb;
}

.app-header,
.app-content {
  width: min(1180px, 100%);
  margin: 0 auto;
  box-sizing: border-box;
}

.app-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 24px;
  padding: 16px 20px 0;
  border: 1px solid #e5e7eb;
  border-radius: 18px;
  background: #ffffff;
  box-shadow: 0 10px 30px rgba(15, 23, 42, 0.06);
}

.app-brand {
  display: flex;
  align-items: center;
  gap: 12px;
  padding-bottom: 16px;
  flex: 0 0 auto;
}

.app-logo {
  display: flex;
  width: 38px;
  height: 38px;
  align-items: center;
  justify-content: center;
  border-radius: 12px;
  background: #1677ff;
  color: #ffffff;
  font-weight: 700;
  letter-spacing: 0.04em;
}

.app-tabs {
  flex: 1;
  min-width: 0;
}

.app-tabs .ant-tabs-nav {
  margin-bottom: 0;
}

.app-tabs .ant-tabs-nav-wrap {
  justify-content: flex-end;
}

.app-content {
  margin-top: 20px;
  padding: 24px;
  border: 1px solid #e5e7eb;
  border-radius: 18px;
  background: #ffffff;
  box-shadow: 0 10px 30px rgba(15, 23, 42, 0.04);
}

.account-page {
  width: 100%;
}

.account-container {
  width: 100%;
}

.account-actions {
  display: flex;
  justify-content: flex-end;
}

.profile-summary {
  display: flex;
  align-items: center;
  gap: 18px;
  margin-bottom: 24px;
}

.account-descriptions {
  margin-top: 8px;
}

@media (max-width: 760px) {
  .app-shell {
    padding: 16px;
  }

  .app-header {
    align-items: stretch;
    flex-direction: column;
    gap: 8px;
    padding: 16px 16px 0;
  }

  .app-tabs .ant-tabs-nav-wrap {
    justify-content: flex-start;
  }

  .app-content {
    padding: 16px;
  }

  .account-actions,
  .profile-summary {
    align-items: flex-start;
    flex-direction: column;
  }
}
```

- [ ] **Step 2: Run TypeScript/build check**

Run:

```bash
npm run build
```

Expected: build succeeds.

---

### Task 4: Manual UI Verification

**Files:**
- No code changes expected.

- [ ] **Step 1: Start the development server**

Run:

```bash
npm run dev
```

Expected: Vite starts and prints a local URL.

- [ ] **Step 2: Verify logged-in shell behavior in the browser**

Use the running app and confirm:

```text
1. If not logged in, login page still appears.
2. After login, the default selected tab is 工作台.
3. Header left side shows FJ logo, Fuck Job title, and AI 求职工作台 subtitle.
4. Clicking 工作台 shows 工作区.
5. Clicking 沟通调试 shows 沟通调试.
6. Clicking 简历优化 shows 简历优化.
7. Clicking 配置中心 shows 配置页面.
8. Clicking 我的 shows account cards for profile, points, and password reset.
9. Clicking 退出登录 on 我的 returns to the login page.
10. Narrowing the window keeps header and tabs usable without horizontal page overflow.
```

- [ ] **Step 3: Stop the development server**

Stop the server with `Ctrl+C` in the terminal running `npm run dev`.

---

## Self-Review

- Spec coverage: The plan covers the unified app shell, logo/title, five tabs, in-window state switching, embedded account page, responsive layout, and build/manual verification.
- Placeholder scan: No placeholders, TODOs, or undefined follow-up implementation steps remain.
- Type consistency: `AppTabKey` values match the spec keys and the `appTabs` metadata used by Ant Design `Tabs`.
