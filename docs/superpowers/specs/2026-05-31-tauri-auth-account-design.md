# Tauri AuthPage 与 AccountPage 设计

## 背景

`fuck_job` 是 Tauri + React 桌面客户端，当前 `src/view/auth/index.tsx` 和 `src/view/account/index.tsx` 只是占位页。后台管理项目为 `RuoYi-Vue-Plus`，已提供登录、注册、图形验证码、邮箱验证码、个人资料、积分和已登录修改密码接口。

目标是在 `fuck_job` 中实现 AuthPage 和 AccountPage。React 页面不直接访问 RuoYi API，所有后端接口对接都在 Tauri Rust command 中完成。

## 范围

实现范围：

- AuthPage 三合一页面：登录、注册、找回密码。
- 登录真实对接后台，包含用户名、密码、图形验证码。
- 注册真实对接后台，包含昵称、用户名、密码、邮箱、邮箱验证码。
- 注册页提供“获取验证码”按钮，对接邮箱验证码接口并倒计时。
- 找回密码只提供表单和前端校验，不实现真实重置功能。
- AccountPage 登录后展示用户资料、头像、邮箱、用户名、积分额度。
- AccountPage 支持已登录用户修改密码。
- 退出登录并返回 AuthPage。
- Rust 端统一处理 API 请求、token、固定参数和加密开关。

不实现范围：

- 未登录邮箱找回密码真实重置。
- 头像上传。
- 个人资料编辑。
- 多租户选择。
- 注册成功后自动登录。
- React Router。

## 页面结构

`App.tsx` 维护登录态。无 token 时展示 `AuthPage`，有 token 时展示 `AccountPage`。当前客户端页面较少，先不引入路由，减少依赖和迁移成本。

`AuthPage` 使用 tab 或 segmented 切换三种模式：

1. 登录
   - 用户名
   - 密码
   - 图形验证码
   - 登录按钮
   - 切换到注册/找回密码
2. 注册
   - 昵称
   - 用户名
   - 密码
   - 确认密码
   - 邮箱
   - 邮箱验证码
   - 获取验证码按钮
   - 注册按钮
3. 找回密码
   - 邮箱
   - 邮箱验证码
   - 新密码
   - 确认新密码
   - 获取验证码按钮
   - 提交后提示“找回密码功能暂未开放”

`AccountPage` 包含：

- 用户资料卡片：头像、昵称、用户名、邮箱、用户 ID、租户 ID。
- 积分卡片：剩余积分 `points`。
- 修改密码表单：旧密码、新密码、确认新密码。
- 退出登录按钮。

## Rust command 边界

React 页面只调用 Tauri `invoke`。Rust 端提供这些 commands：

- `get_captcha()`：请求 `GET /auth/code`，返回验证码图片、`uuid` 等登录所需信息。
- `login(input)`：请求 `POST /login`，返回 token 和必要登录信息。
- `register(input)`：请求 `POST /register`。
- `send_email_code(email)`：请求 `GET /resource/email/code?email=...`。
- `get_account_profile()`：请求 `GET /system/user/profile`。
- `get_my_points()`：请求 `GET /system/points/my`。
- `update_password(input)`：请求 `PUT /system/user/profile/updatePwd`。
- `logout()`：请求 `POST /logout`，并清理本地 token。

固定参数由 Rust 端注入：

- `tenantId = "000000"`
- `clientId` 使用 Rust 配置或常量维护。
- `grantType = "password"` 用于密码登录。

Rust 端负责保存和读取 token，并在需要登录态的接口上携带后台要求的 token header。

## API 契约

对接现有后台接口：

- `GET /auth/code`
  - 用于登录图形验证码。
  - 登录时提交 `code` 和 `uuid`。
- `POST /login`
  - 请求字段包括 `clientId`、`grantType`、`tenantId`、`username`、`password`、`code`、`uuid`。
  - 请求体需要支持 RuoYi `@ApiEncrypt`。
- `GET /resource/email/code?email=...`
  - 用于注册和找回密码表单的验证码按钮。
- `POST /register`
  - 请求字段包括 `clientId`、`grantType`、`tenantId`、`nickname`、`username`、`password`、`email`、`emailCode`。
  - 用户名限制为英文字母和数字。
  - 请求体需要支持 RuoYi `@ApiEncrypt`。
- `GET /system/user/profile`
  - 返回个人中心资料。
- `GET /system/points/my`
  - 返回当前用户积分余额。
- `PUT /system/user/profile/updatePwd`
  - 请求字段包括 `oldPassword`、`newPassword`。
  - 请求体需要支持 RuoYi `@ApiEncrypt`。
- `POST /logout`
  - 后台登出并清理本地登录态。

## 加密策略

加密逻辑只放在 Rust 端 API 层。

- dev 模式默认不加密，便于本地调试。
- 非 dev 模式对带 `@ApiEncrypt` 的请求体执行 RuoYi 兼容加密。
- React 不感知加密实现，只接收 command 成功或失败结果。

## 状态与数据流

登录流程：

1. AuthPage 调用 `get_captcha()`，显示验证码。
2. 用户输入用户名、密码、验证码。
3. AuthPage 调用 `login()`。
4. Rust 注入固定参数、按环境加密、请求后台。
5. 登录成功后 Rust 保存 token，React 切换到 AccountPage。
6. 登录失败后刷新图形验证码并展示错误。

注册流程：

1. 用户输入邮箱后点击“获取验证码”。
2. AuthPage 调用 `send_email_code(email)`。
3. 成功后按钮倒计时并禁用。
4. 用户填写注册信息并提交。
5. AuthPage 调用 `register()`。
6. 成功后提示注册成功，切回登录 tab 并刷新验证码。

账户流程：

1. AccountPage 挂载时并行调用 `get_account_profile()` 和 `get_my_points()`。
2. 个人资料成功后展示头像、用户名、邮箱等。
3. 积分接口失败不阻塞资料展示，只在积分卡片显示失败状态。
4. 修改密码通过 `update_password()` 提交。
5. 退出登录调用 `logout()`，清理 token 后返回 AuthPage。

找回密码流程：

1. 用户填写邮箱、邮箱验证码、新密码、确认新密码。
2. 前端校验邮箱格式、验证码非空、两次密码一致。
3. 提交后提示“找回密码功能暂未开放”。
4. 不调用真实重置密码接口。

## 错误处理

Rust 端统一解析 RuoYi `R` 响应：

- 成功响应返回 typed data。
- 失败响应提取后台消息。
- 网络错误、JSON 解析错误、token 缺失、token 过期统一转成前端可展示字符串。

React 端统一展示：

- 表单校验错误显示在字段或页面提示中。
- 后台错误使用 antd `message` 或 `Alert`。
- 登录失败刷新图形验证码。
- token 过期时清理登录态并返回 AuthPage。
- 积分加载失败只影响积分卡片。

## UI 约束

优先使用已有依赖 `antd` 与普通 CSS，不新增 UI 框架。页面适配桌面窗口，保持简洁：认证页居中卡片，账户页使用卡片分区。

头像展示优先使用后台返回的头像 URL；没有头像时显示用户名或昵称首字母占位。

## 验证计划

实现后验证：

1. 在 `fuck_job` 运行 TypeScript/Vite build，确保 React 代码无类型错误。
2. 编译 Tauri Rust 端，确保 commands 和 API 客户端无 Rust 编译错误。
3. dev 模式手动检查 AuthPage：验证码加载、登录失败提示、注册表单校验、邮箱验证码按钮倒计时、找回密码占位提示。
4. 后台可用时手动检查真实流程：登录成功进入 AccountPage、个人资料加载、积分加载、修改密码提示、退出登录返回 AuthPage。
