# Tauri AuthPage and AccountPage Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Tauri-backed AuthPage and AccountPage for login, registration, account display, points display, password update, and logout against the RuoYi-Vue-Plus backend.

**Architecture:** React renders forms and account UI, but never calls RuoYi directly. Tauri Rust commands own HTTP calls, fixed RuoYi parameters, token storage, response normalization, and dev/prod API encryption. The app keeps a simple token-driven view switch in `App.tsx` instead of introducing routing.

**Tech Stack:** Tauri 2, Rust, reqwest, serde, keyring, React 19, TypeScript, antd 6, Vite.

---

## File Structure

- Modify `src-tauri/Cargo.toml`: add Rust dependencies needed for AES/RSA encryption and random AES key generation.
- Create `src-tauri/src/auth_api.rs`: RuoYi config constants, request/response DTOs, HTTP client helpers, encryption helper, token storage helper, and Tauri commands.
- Modify `src-tauri/src/lib.rs`: register the new Tauri commands and remove the demo `greet` command.
- Create `src/types/auth.ts`: shared TypeScript types for Tauri command inputs and outputs.
- Create `src/lib/tauriAuth.ts`: typed wrappers around `invoke`.
- Modify `src/App.tsx`: replace starter demo with auth/account shell and login-state handling.
- Modify `src/view/auth/index.tsx`: implement login/register/forgot-password UI.
- Modify `src/view/account/index.tsx`: implement profile, points, password update, and logout UI.
- Modify `src/App.css`: replace starter styles with app/auth/account layout styles.

No backend files are modified in this plan.

---

## Task 1: Add Rust dependencies

**Files:**
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: Add dependencies**

Add these dependencies under `[dependencies]` in `src-tauri/Cargo.toml`:

```toml
aes = "0.8"
ecb = { version = "0.1", features = ["alloc", "block-padding"] }
rand = "0.8"
rsa = { version = "0.9", features = ["pem"] }
sha2 = "0.10"
```

`reqwest`, `serde`, `serde_json`, `base64`, `keyring`, and `thiserror` already exist.

- [ ] **Step 2: Check dependency resolution**

Run:

```bash
pnpm tauri info
```

Expected: command completes and Cargo can resolve dependencies. If the command prints environment information and exits successfully, proceed.

- [ ] **Step 3: Commit if inside a git repo**

Current parent directory is not a git repository. If execution happens inside a git repository, run:

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "chore: add auth api rust dependencies"
```

Expected: a new commit is created. If not in a git repository, skip this step and record that commit was skipped.

---

## Task 2: Implement Rust RuoYi API commands

**Files:**
- Create: `src-tauri/src/auth_api.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Create `src-tauri/src/auth_api.rs`**

Create the file with this implementation:

```rust
use aes::Aes128;
use base64::{engine::general_purpose, Engine as _};
use ecb::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyInit};
use rand::{distributions::Alphanumeric, Rng};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use rsa::{pkcs8::DecodePublicKey, Pkcs1v15Encrypt, RsaPublicKey};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Sha256;
use thiserror::Error;

type Aes128EcbEnc = ecb::Encryptor<Aes128>;

const BASE_URL: &str = "http://localhost:8080";
const TENANT_ID: &str = "000000";
const CLIENT_ID: &str = "e5cd7e4891bf95d1d19206ce24a7b32e";
const GRANT_TYPE_PASSWORD: &str = "password";
const TOKEN_SERVICE: &str = "fuck_job";
const TOKEN_USER: &str = "ruoyi_token";
const ENCRYPT_HEADER: &str = "encrypt-key";
const RQ_PUBLIC_KEY: &str = "MFwwDQYJKoZIhvcNAQEBBQADSwAwSAJBAKoR8mX0rGKLqzcWmOzbfj64K8ZIgOdHnzkXSOVOZbFu/TJhZ7rFAN+eaGkl3C4buccQd/EjEsj9ir7ijT7h96MCAwEAAQ==";

#[derive(Debug, Error)]
pub enum AuthApiError {
    #[error("网络请求失败：{0}")]
    Request(#[from] reqwest::Error),
    #[error("后台响应解析失败：{0}")]
    Json(#[from] serde_json::Error),
    #[error("请求加密失败：{0}")]
    Crypto(String),
    #[error("登录状态已失效，请重新登录")]
    MissingToken,
    #[error("凭证存储失败：{0}")]
    Keyring(String),
    #[error("{0}")]
    Backend(String),
}

impl Serialize for AuthApiError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

#[derive(Debug, Deserialize)]
struct RuoYiResponse<T> {
    code: i32,
    msg: Option<String>,
    data: Option<T>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptchaResponse {
    pub captcha_enabled: Option<bool>,
    pub uuid: Option<String>,
    pub img: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoginResponse {
    access_token: Option<String>,
    token: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthSession {
    pub token: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginInput {
    pub username: String,
    pub password: String,
    pub code: String,
    pub uuid: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterInput {
    pub nickname: String,
    pub username: String,
    pub password: String,
    pub email: String,
    pub email_code: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePasswordInput {
    pub old_password: String,
    pub new_password: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProfileUser {
    pub user_id: Option<u64>,
    pub tenant_id: Option<String>,
    pub user_name: Option<String>,
    pub nick_name: Option<String>,
    pub email: Option<String>,
    pub avatar: Option<Value>,
    pub dept_name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountProfile {
    pub user: ProfileUser,
    pub role_group: Option<String>,
    pub post_group: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserPoints {
    pub user_id: Option<u64>,
    pub points: Option<i64>,
    pub remark: Option<String>,
}

#[tauri::command]
pub async fn get_captcha() -> Result<CaptchaResponse, AuthApiError> {
    get_json("/auth/code", false).await
}

#[tauri::command]
pub async fn login(input: LoginInput) -> Result<AuthSession, AuthApiError> {
    let body = json!({
        "clientId": CLIENT_ID,
        "grantType": GRANT_TYPE_PASSWORD,
        "tenantId": TENANT_ID,
        "username": input.username,
        "password": input.password,
        "code": input.code,
        "uuid": input.uuid,
    });
    let response: LoginResponse = post_json("/login", body, false, true).await?;
    let token = response
        .access_token
        .or(response.token)
        .ok_or_else(|| AuthApiError::Backend("登录成功但后台未返回 token".to_string()))?;
    save_token(&token)?;
    Ok(AuthSession { token })
}

#[tauri::command]
pub async fn register(input: RegisterInput) -> Result<(), AuthApiError> {
    let body = json!({
        "clientId": CLIENT_ID,
        "grantType": GRANT_TYPE_PASSWORD,
        "tenantId": TENANT_ID,
        "nickname": input.nickname,
        "username": input.username,
        "password": input.password,
        "email": input.email,
        "emailCode": input.email_code,
    });
    post_empty("/register", body, false, true).await
}

#[tauri::command]
pub async fn send_email_code(email: String) -> Result<(), AuthApiError> {
    let encoded = urlencoding::encode(&email);
    get_empty(&format!("/resource/email/code?email={encoded}"), false).await
}

#[tauri::command]
pub async fn get_account_profile() -> Result<AccountProfile, AuthApiError> {
    get_json("/system/user/profile", true).await
}

#[tauri::command]
pub async fn get_my_points() -> Result<Option<UserPoints>, AuthApiError> {
    get_json("/system/points/my", true).await
}

#[tauri::command]
pub async fn update_password(input: UpdatePasswordInput) -> Result<(), AuthApiError> {
    let body = json!({
        "oldPassword": input.old_password,
        "newPassword": input.new_password,
    });
    post_empty_with_method(reqwest::Method::PUT, "/system/user/profile/updatePwd", body, true, true).await
}

#[tauri::command]
pub async fn logout() -> Result<(), AuthApiError> {
    let result = post_empty("/logout", json!({}), true, false).await;
    clear_token()?;
    result
}

#[tauri::command]
pub fn current_session() -> Result<Option<AuthSession>, AuthApiError> {
    Ok(load_token()?.map(|token| AuthSession { token }))
}

async fn get_json<T: DeserializeOwned>(path: &str, auth: bool) -> Result<T, AuthApiError> {
    let response: RuoYiResponse<T> = client()
        .get(url(path))
        .headers(headers(auth)?)
        .send()
        .await?
        .json()
        .await?;
    unwrap_ruoyi(response)
}

async fn get_empty(path: &str, auth: bool) -> Result<(), AuthApiError> {
    let response: RuoYiResponse<Value> = client()
        .get(url(path))
        .headers(headers(auth)?)
        .send()
        .await?
        .json()
        .await?;
    unwrap_ruoyi_empty(response)
}

async fn post_json<T: DeserializeOwned>(path: &str, body: Value, auth: bool, encryptable: bool) -> Result<T, AuthApiError> {
    let (payload, encryption_header) = request_payload(body, encryptable)?;
    let mut headers = headers(auth)?;
    if let Some(value) = encryption_header {
        headers.insert(ENCRYPT_HEADER, HeaderValue::from_str(&value).map_err(|error| AuthApiError::Crypto(error.to_string()))?);
    }
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    let response: RuoYiResponse<T> = client()
        .post(url(path))
        .headers(headers)
        .body(payload)
        .send()
        .await?
        .json()
        .await?;
    unwrap_ruoyi(response)
}

async fn post_empty(path: &str, body: Value, auth: bool, encryptable: bool) -> Result<(), AuthApiError> {
    post_empty_with_method(reqwest::Method::POST, path, body, auth, encryptable).await
}

async fn post_empty_with_method(method: reqwest::Method, path: &str, body: Value, auth: bool, encryptable: bool) -> Result<(), AuthApiError> {
    let (payload, encryption_header) = request_payload(body, encryptable)?;
    let mut headers = headers(auth)?;
    if let Some(value) = encryption_header {
        headers.insert(ENCRYPT_HEADER, HeaderValue::from_str(&value).map_err(|error| AuthApiError::Crypto(error.to_string()))?);
    }
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    let response: RuoYiResponse<Value> = client()
        .request(method, url(path))
        .headers(headers)
        .body(payload)
        .send()
        .await?
        .json()
        .await?;
    unwrap_ruoyi_empty(response)
}

fn request_payload(body: Value, encryptable: bool) -> Result<(String, Option<String>), AuthApiError> {
    let plain = serde_json::to_string(&body)?;
    if cfg!(debug_assertions) || !encryptable {
        return Ok((plain, None));
    }
    encrypt_request_body(&plain).map(|(payload, header)| (payload, Some(header)))
}

fn encrypt_request_body(plain: &str) -> Result<(String, String), AuthApiError> {
    let aes_key: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(16)
        .map(char::from)
        .collect();
    let encrypted_body = Aes128EcbEnc::new_from_slice(aes_key.as_bytes())
        .map_err(|error| AuthApiError::Crypto(error.to_string()))?
        .encrypt_padded_vec_mut::<Pkcs7>(plain.as_bytes());
    let payload = general_purpose::STANDARD.encode(encrypted_body);
    let base64_key = general_purpose::STANDARD.encode(aes_key.as_bytes());
    let public_key_der = general_purpose::STANDARD
        .decode(RQ_PUBLIC_KEY)
        .map_err(|error| AuthApiError::Crypto(error.to_string()))?;
    let public_key = RsaPublicKey::from_public_key_der(&public_key_der)
        .map_err(|error| AuthApiError::Crypto(error.to_string()))?;
    let encrypted_key = public_key
        .encrypt(&mut rand::thread_rng(), Pkcs1v15Encrypt, base64_key.as_bytes())
        .map_err(|error| AuthApiError::Crypto(error.to_string()))?;
    Ok((payload, general_purpose::STANDARD.encode(encrypted_key)))
}

fn unwrap_ruoyi<T>(response: RuoYiResponse<T>) -> Result<T, AuthApiError> {
    if response.code == 200 {
        response.data.ok_or_else(|| AuthApiError::Backend("后台响应缺少 data".to_string()))
    } else {
        Err(AuthApiError::Backend(response.msg.unwrap_or_else(|| "后台请求失败".to_string())))
    }
}

fn unwrap_ruoyi_empty(response: RuoYiResponse<Value>) -> Result<(), AuthApiError> {
    if response.code == 200 {
        Ok(())
    } else {
        Err(AuthApiError::Backend(response.msg.unwrap_or_else(|| "后台请求失败".to_string())))
    }
}

fn headers(auth: bool) -> Result<HeaderMap, AuthApiError> {
    let mut headers = HeaderMap::new();
    if auth {
        let token = load_token()?.ok_or(AuthApiError::MissingToken)?;
        headers.insert("Authorization", HeaderValue::from_str(&format!("Bearer {token}")).map_err(|error| AuthApiError::Backend(error.to_string()))?);
    }
    Ok(headers)
}

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

fn url(path: &str) -> String {
    format!("{BASE_URL}{path}")
}

fn token_entry() -> Result<keyring::Entry, AuthApiError> {
    keyring::Entry::new(TOKEN_SERVICE, TOKEN_USER).map_err(|error| AuthApiError::Keyring(error.to_string()))
}

fn save_token(token: &str) -> Result<(), AuthApiError> {
    token_entry()?.set_password(token).map_err(|error| AuthApiError::Keyring(error.to_string()))
}

fn load_token() -> Result<Option<String>, AuthApiError> {
    match token_entry()?.get_password() {
        Ok(token) => Ok(Some(token)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(AuthApiError::Keyring(error.to_string())),
    }
}

fn clear_token() -> Result<(), AuthApiError> {
    match token_entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(AuthApiError::Keyring(error.to_string())),
    }
}
```

- [ ] **Step 2: Register commands in `src-tauri/src/lib.rs`**

Replace the file contents with:

```rust
mod auth_api;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            auth_api::current_session,
            auth_api::get_captcha,
            auth_api::login,
            auth_api::register,
            auth_api::send_email_code,
            auth_api::get_account_profile,
            auth_api::get_my_points,
            auth_api::update_password,
            auth_api::logout,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 3: Check Rust compilation**

Run:

```bash
pnpm tauri build --debug
```

Expected: Rust compile succeeds. If the frontend build fails because React code has not been implemented yet, the Rust compiler output should still not contain errors from `auth_api.rs` or `lib.rs`.

- [ ] **Step 4: Commit if inside a git repo**

```bash
git add src-tauri/src/auth_api.rs src-tauri/src/lib.rs src-tauri/Cargo.lock
git commit -m "feat: add tauri auth api commands"
```

Expected: a new commit is created. If not in a git repository, skip this step and record that commit was skipped.

---

## Task 3: Add TypeScript command wrappers

**Files:**
- Create: `src/types/auth.ts`
- Create: `src/lib/tauriAuth.ts`

- [ ] **Step 1: Create `src/types/auth.ts`**

```ts
export interface CaptchaResponse {
  captchaEnabled?: boolean;
  uuid?: string;
  img?: string;
}

export interface AuthSession {
  token: string;
}

export interface LoginInput {
  username: string;
  password: string;
  code: string;
  uuid: string;
}

export interface RegisterInput {
  nickname: string;
  username: string;
  password: string;
  email: string;
  emailCode: string;
}

export interface UpdatePasswordInput {
  oldPassword: string;
  newPassword: string;
}

export interface ProfileUser {
  userId?: number;
  tenantId?: string;
  userName?: string;
  nickName?: string;
  email?: string;
  avatar?: string | number | null;
  deptName?: string;
}

export interface AccountProfile {
  user: ProfileUser;
  roleGroup?: string;
  postGroup?: string;
}

export interface UserPoints {
  userId?: number;
  points?: number;
  remark?: string;
}
```

- [ ] **Step 2: Create `src/lib/tauriAuth.ts`**

```ts
import { invoke } from "@tauri-apps/api/core";
import type {
  AccountProfile,
  AuthSession,
  CaptchaResponse,
  LoginInput,
  RegisterInput,
  UpdatePasswordInput,
  UserPoints,
} from "../types/auth";

export function getCaptcha(): Promise<CaptchaResponse> {
  return invoke<CaptchaResponse>("get_captcha");
}

export function login(input: LoginInput): Promise<AuthSession> {
  return invoke<AuthSession>("login", { input });
}

export function register(input: RegisterInput): Promise<void> {
  return invoke<void>("register", { input });
}

export function sendEmailCode(email: string): Promise<void> {
  return invoke<void>("send_email_code", { email });
}

export function getAccountProfile(): Promise<AccountProfile> {
  return invoke<AccountProfile>("get_account_profile");
}

export function getMyPoints(): Promise<UserPoints | null> {
  return invoke<UserPoints | null>("get_my_points");
}

export function updatePassword(input: UpdatePasswordInput): Promise<void> {
  return invoke<void>("update_password", { input });
}

export function logout(): Promise<void> {
  return invoke<void>("logout");
}

export function currentSession(): Promise<AuthSession | null> {
  return invoke<AuthSession | null>("current_session");
}

export function getErrorMessage(error: unknown): string {
  if (typeof error === "string") {
    return error;
  }

  if (error instanceof Error) {
    return error.message;
  }

  return "操作失败";
}
```

- [ ] **Step 3: Run TypeScript check**

Run:

```bash
pnpm build
```

Expected: it may still build the starter UI if later tasks are not done. There should be no TypeScript errors in `src/types/auth.ts` or `src/lib/tauriAuth.ts`.

- [ ] **Step 4: Commit if inside a git repo**

```bash
git add src/types/auth.ts src/lib/tauriAuth.ts
git commit -m "feat: add typed auth command wrappers"
```

Expected: a new commit is created. If not in a git repository, skip this step and record that commit was skipped.

---

## Task 4: Replace `App.tsx` with auth/account shell

**Files:**
- Modify: `src/App.tsx`

- [ ] **Step 1: Replace `src/App.tsx`**

```tsx
import { useEffect, useState } from "react";
import { Spin } from "antd";
import "./App.css";
import AccountPage from "./view/account";
import AuthPage from "./view/auth";
import { currentSession } from "./lib/tauriAuth";
import type { AuthSession } from "./types/auth";

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
    <AccountPage onLoggedOut={() => setSession(null)} />
  ) : (
    <AuthPage onLoggedIn={setSession} />
  );
}

export default App;
```

- [ ] **Step 2: Run TypeScript check**

Run:

```bash
pnpm build
```

Expected: TypeScript fails because `AuthPage` and `AccountPage` props are not implemented yet. The expected errors mention `onLoggedIn` or `onLoggedOut` props.

- [ ] **Step 3: Commit if inside a git repo after Task 5 and Task 6 pass**

Do not commit `App.tsx` by itself if the build is red. Commit it with Task 5 and Task 6 once the app compiles.

---

## Task 5: Implement AuthPage

**Files:**
- Modify: `src/view/auth/index.tsx`

- [ ] **Step 1: Replace `src/view/auth/index.tsx`**

```tsx
import { useEffect, useState } from "react";
import { Alert, Button, Card, Form, Input, Segmented, Space, Typography, message } from "antd";
import type { AuthSession } from "../../types/auth";
import { getCaptcha, getErrorMessage, login, register, sendEmailCode } from "../../lib/tauriAuth";

interface AuthPageProps {
  onLoggedIn: (session: AuthSession) => void;
}

type AuthMode = "login" | "register" | "forgot";

interface LoginFormValues {
  username: string;
  password: string;
  code: string;
}

interface RegisterFormValues {
  nickname: string;
  username: string;
  password: string;
  confirmPassword: string;
  email: string;
  emailCode: string;
}

interface ForgotFormValues {
  email: string;
  emailCode: string;
  password: string;
  confirmPassword: string;
}

const usernamePattern = /^[A-Za-z0-9]+$/;

const AuthPage = ({ onLoggedIn }: AuthPageProps) => {
  const [mode, setMode] = useState<AuthMode>("login");
  const [captchaImg, setCaptchaImg] = useState<string>();
  const [captchaUuid, setCaptchaUuid] = useState<string>();
  const [captchaLoading, setCaptchaLoading] = useState(false);
  const [loginLoading, setLoginLoading] = useState(false);
  const [registerLoading, setRegisterLoading] = useState(false);
  const [emailCountdown, setEmailCountdown] = useState(0);
  const [activeEmail, setActiveEmail] = useState("");
  const [messageApi, contextHolder] = message.useMessage();
  const [loginForm] = Form.useForm<LoginFormValues>();
  const [registerForm] = Form.useForm<RegisterFormValues>();
  const [forgotForm] = Form.useForm<ForgotFormValues>();

  useEffect(() => {
    void refreshCaptcha();
  }, []);

  useEffect(() => {
    if (emailCountdown <= 0) {
      return;
    }

    const timer = window.setTimeout(() => setEmailCountdown((value) => value - 1), 1000);
    return () => window.clearTimeout(timer);
  }, [emailCountdown]);

  async function refreshCaptcha(): Promise<void> {
    setCaptchaLoading(true);
    try {
      const captcha = await getCaptcha();
      setCaptchaImg(captcha.img);
      setCaptchaUuid(captcha.uuid);
      loginForm.setFieldValue("code", "");
    } catch (error: unknown) {
      messageApi.error(getErrorMessage(error));
    } finally {
      setCaptchaLoading(false);
    }
  }

  async function submitLogin(values: LoginFormValues): Promise<void> {
    if (!captchaUuid) {
      messageApi.error("验证码未加载，请刷新验证码");
      return;
    }

    setLoginLoading(true);
    try {
      const session = await login({ ...values, uuid: captchaUuid });
      messageApi.success("登录成功");
      onLoggedIn(session);
    } catch (error: unknown) {
      messageApi.error(getErrorMessage(error));
      void refreshCaptcha();
    } finally {
      setLoginLoading(false);
    }
  }

  async function submitRegister(values: RegisterFormValues): Promise<void> {
    setRegisterLoading(true);
    try {
      await register({
        nickname: values.nickname,
        username: values.username,
        password: values.password,
        email: values.email,
        emailCode: values.emailCode,
      });
      messageApi.success("注册成功，请登录");
      registerForm.resetFields();
      setMode("login");
      await refreshCaptcha();
    } catch (error: unknown) {
      messageApi.error(getErrorMessage(error));
    } finally {
      setRegisterLoading(false);
    }
  }

  async function requestEmailCode(form: "register" | "forgot"): Promise<void> {
    const targetForm = form === "register" ? registerForm : forgotForm;
    const email = targetForm.getFieldValue("email");
    if (!email) {
      messageApi.error("请先填写邮箱");
      return;
    }

    try {
      await targetForm.validateFields(["email"]);
      await sendEmailCode(email);
      setActiveEmail(email);
      setEmailCountdown(60);
      messageApi.success("邮箱验证码已发送");
    } catch (error: unknown) {
      messageApi.error(getErrorMessage(error));
    }
  }

  function submitForgot(): void {
    messageApi.info("找回密码功能暂未开放");
  }

  const emailButtonText = emailCountdown > 0 ? `${emailCountdown}s 后重试` : "获取验证码";

  return (
    <main className="auth-page">
      {contextHolder}
      <Card className="auth-card">
        <Space direction="vertical" size="large" className="full-width">
          <div className="auth-header">
            <Typography.Title level={2}>fuck_job</Typography.Title>
            <Typography.Text type="secondary">登录后管理账号、积分与简历优化能力</Typography.Text>
          </div>

          <Segmented
            block
            value={mode}
            onChange={(value) => setMode(value as AuthMode)}
            options={[
              { label: "登录", value: "login" },
              { label: "注册", value: "register" },
              { label: "找回密码", value: "forgot" },
            ]}
          />

          {mode === "login" && (
            <Form form={loginForm} layout="vertical" onFinish={submitLogin} autoComplete="off">
              <Form.Item name="username" label="用户名" rules={[{ required: true, message: "请输入用户名" }]}>
                <Input placeholder="请输入用户名" />
              </Form.Item>
              <Form.Item name="password" label="密码" rules={[{ required: true, message: "请输入密码" }]}>
                <Input.Password placeholder="请输入密码" />
              </Form.Item>
              <Form.Item label="图形验证码" required>
                <Space.Compact className="full-width">
                  <Form.Item name="code" noStyle rules={[{ required: true, message: "请输入验证码" }]}>
                    <Input placeholder="请输入验证码" />
                  </Form.Item>
                  <Button loading={captchaLoading} onClick={() => void refreshCaptcha()}>
                    {captchaImg ? <img className="captcha-image" src={`data:image/gif;base64,${captchaImg}`} alt="验证码" /> : "刷新"}
                  </Button>
                </Space.Compact>
              </Form.Item>
              <Button type="primary" htmlType="submit" block loading={loginLoading}>
                登录
              </Button>
            </Form>
          )}

          {mode === "register" && (
            <Form form={registerForm} layout="vertical" onFinish={submitRegister} autoComplete="off">
              <Form.Item name="nickname" label="昵称" rules={[{ required: true, message: "请输入昵称" }, { min: 2, max: 30, message: "昵称长度为 2-30 位" }]}>
                <Input placeholder="请输入昵称" />
              </Form.Item>
              <Form.Item name="username" label="用户名" rules={[{ required: true, message: "请输入用户名" }, { pattern: usernamePattern, message: "用户名只能包含英文字母和数字" }]}>
                <Input placeholder="英文字母和数字" />
              </Form.Item>
              <Form.Item name="password" label="密码" rules={[{ required: true, message: "请输入密码" }, { min: 5, max: 30, message: "密码长度为 5-30 位" }]}>
                <Input.Password placeholder="请输入密码" />
              </Form.Item>
              <Form.Item name="confirmPassword" label="确认密码" dependencies={["password"]} rules={[{ required: true, message: "请再次输入密码" }, ({ getFieldValue }) => ({ validator(_, value) { return !value || getFieldValue("password") === value ? Promise.resolve() : Promise.reject(new Error("两次密码不一致")); } })]}>
                <Input.Password placeholder="请再次输入密码" />
              </Form.Item>
              <Form.Item name="email" label="邮箱" rules={[{ required: true, message: "请输入邮箱" }, { type: "email", message: "邮箱格式不正确" }]}>
                <Input placeholder="请输入邮箱" />
              </Form.Item>
              <Form.Item label="邮箱验证码" required>
                <Space.Compact className="full-width">
                  <Form.Item name="emailCode" noStyle rules={[{ required: true, message: "请输入邮箱验证码" }]}>
                    <Input placeholder="请输入邮箱验证码" />
                  </Form.Item>
                  <Button disabled={emailCountdown > 0 && activeEmail === registerForm.getFieldValue("email")} onClick={() => void requestEmailCode("register")}>
                    {emailButtonText}
                  </Button>
                </Space.Compact>
              </Form.Item>
              <Button type="primary" htmlType="submit" block loading={registerLoading}>
                注册
              </Button>
            </Form>
          )}

          {mode === "forgot" && (
            <Form form={forgotForm} layout="vertical" onFinish={submitForgot} autoComplete="off">
              <Alert type="info" showIcon message="找回密码功能暂未开放，当前仅提供表单入口。" />
              <Form.Item name="email" label="邮箱" rules={[{ required: true, message: "请输入邮箱" }, { type: "email", message: "邮箱格式不正确" }]}>
                <Input placeholder="请输入邮箱" />
              </Form.Item>
              <Form.Item label="邮箱验证码" required>
                <Space.Compact className="full-width">
                  <Form.Item name="emailCode" noStyle rules={[{ required: true, message: "请输入邮箱验证码" }]}>
                    <Input placeholder="请输入邮箱验证码" />
                  </Form.Item>
                  <Button disabled={emailCountdown > 0 && activeEmail === forgotForm.getFieldValue("email")} onClick={() => void requestEmailCode("forgot")}>
                    {emailButtonText}
                  </Button>
                </Space.Compact>
              </Form.Item>
              <Form.Item name="password" label="新密码" rules={[{ required: true, message: "请输入新密码" }, { min: 5, max: 30, message: "密码长度为 5-30 位" }]}>
                <Input.Password placeholder="请输入新密码" />
              </Form.Item>
              <Form.Item name="confirmPassword" label="确认新密码" dependencies={["password"]} rules={[{ required: true, message: "请再次输入新密码" }, ({ getFieldValue }) => ({ validator(_, value) { return !value || getFieldValue("password") === value ? Promise.resolve() : Promise.reject(new Error("两次密码不一致")); } })]}>
                <Input.Password placeholder="请再次输入新密码" />
              </Form.Item>
              <Button type="primary" htmlType="submit" block>
                提交
              </Button>
            </Form>
          )}
        </Space>
      </Card>
    </main>
  );
};

export default AuthPage;
```

- [ ] **Step 2: Run TypeScript check**

Run:

```bash
pnpm build
```

Expected: TypeScript may still fail because `AccountPage` props are not implemented. No TypeScript errors should point to `src/view/auth/index.tsx`.

---

## Task 6: Implement AccountPage

**Files:**
- Modify: `src/view/account/index.tsx`

- [ ] **Step 1: Replace `src/view/account/index.tsx`**

```tsx
import { useEffect, useState } from "react";
import { Avatar, Button, Card, Descriptions, Form, Input, Skeleton, Space, Statistic, Typography, message } from "antd";
import type { AccountProfile, UserPoints } from "../../types/auth";
import { getAccountProfile, getErrorMessage, getMyPoints, logout, updatePassword } from "../../lib/tauriAuth";

interface AccountPageProps {
  onLoggedOut: () => void;
}

interface PasswordFormValues {
  oldPassword: string;
  newPassword: string;
  confirmPassword: string;
}

const AccountPage = ({ onLoggedOut }: AccountPageProps) => {
  const [profile, setProfile] = useState<AccountProfile>();
  const [points, setPoints] = useState<UserPoints | null>();
  const [profileLoading, setProfileLoading] = useState(true);
  const [pointsLoading, setPointsLoading] = useState(true);
  const [passwordLoading, setPasswordLoading] = useState(false);
  const [messageApi, contextHolder] = message.useMessage();
  const [form] = Form.useForm<PasswordFormValues>();

  useEffect(() => {
    void loadAccount();
  }, []);

  async function loadAccount(): Promise<void> {
    setProfileLoading(true);
    setPointsLoading(true);

    try {
      const account = await getAccountProfile();
      setProfile(account);
    } catch (error: unknown) {
      messageApi.error(getErrorMessage(error));
      onLoggedOut();
    } finally {
      setProfileLoading(false);
    }

    try {
      const value = await getMyPoints();
      setPoints(value);
    } catch (error: unknown) {
      setPoints(null);
      messageApi.warning(getErrorMessage(error));
    } finally {
      setPointsLoading(false);
    }
  }

  async function submitPassword(values: PasswordFormValues): Promise<void> {
    setPasswordLoading(true);
    try {
      await updatePassword({ oldPassword: values.oldPassword, newPassword: values.newPassword });
      messageApi.success("密码修改成功");
      form.resetFields();
    } catch (error: unknown) {
      messageApi.error(getErrorMessage(error));
    } finally {
      setPasswordLoading(false);
    }
  }

  async function submitLogout(): Promise<void> {
    try {
      await logout();
    } catch (error: unknown) {
      messageApi.warning(getErrorMessage(error));
    } finally {
      onLoggedOut();
    }
  }

  const user = profile?.user;
  const displayName = user?.nickName || user?.userName || "用户";
  const avatarText = displayName.slice(0, 1).toUpperCase();
  const avatarUrl = typeof user?.avatar === "string" ? user.avatar : undefined;

  return (
    <main className="account-page">
      {contextHolder}
      <Space direction="vertical" size="large" className="account-container">
        <div className="account-topbar">
          <div>
            <Typography.Title level={2}>账户中心</Typography.Title>
            <Typography.Text type="secondary">查看账号信息、积分额度并修改密码</Typography.Text>
          </div>
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
    </main>
  );
};

export default AccountPage;
```

- [ ] **Step 2: Run build**

Run:

```bash
pnpm build
```

Expected: TypeScript and Vite build pass.

- [ ] **Step 3: Commit if inside a git repo**

```bash
git add src/App.tsx src/view/auth/index.tsx src/view/account/index.tsx src/types/auth.ts src/lib/tauriAuth.ts
git commit -m "feat: build auth and account pages"
```

Expected: a new commit is created. If not in a git repository, skip this step and record that commit was skipped.

---

## Task 7: Replace starter CSS

**Files:**
- Modify: `src/App.css`

- [ ] **Step 1: Replace `src/App.css`**

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
.account-page {
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
.account-topbar h2,
.profile-summary h3 {
  margin: 0;
}

.captcha-image {
  display: block;
  width: 96px;
  height: 30px;
  object-fit: contain;
}

.account-page {
  padding: 32px;
  box-sizing: border-box;
  background: #f5f7fb;
}

.account-container {
  width: min(980px, 100%);
  margin: 0 auto;
}

.account-topbar {
  display: flex;
  justify-content: space-between;
  gap: 16px;
  align-items: center;
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

@media (max-width: 640px) {
  .account-page {
    padding: 16px;
  }

  .account-topbar,
  .profile-summary {
    align-items: flex-start;
    flex-direction: column;
  }
}
```

- [ ] **Step 2: Run build**

Run:

```bash
pnpm build
```

Expected: build passes.

- [ ] **Step 3: Commit if inside a git repo**

```bash
git add src/App.css
git commit -m "refactor: replace starter app styles"
```

Expected: a new commit is created. If not in a git repository, skip this step and record that commit was skipped.

---

## Task 8: Verify desktop behavior

**Files:**
- No code changes expected unless verification finds defects.

- [ ] **Step 1: Run frontend build**

Run:

```bash
pnpm build
```

Expected: `tsc && vite build` completes successfully.

- [ ] **Step 2: Run Tauri debug build**

Run:

```bash
pnpm tauri build --debug
```

Expected: Rust and frontend build complete successfully.

- [ ] **Step 3: Start dev app**

Run:

```bash
pnpm tauri dev
```

Expected: desktop window opens and displays AuthPage.

- [ ] **Step 4: Manual AuthPage checks**

Check these flows:

1. Login tab loads a graph captcha image.
2. Clicking captcha refresh button requests a new captcha.
3. Submitting empty login form shows field validation messages.
4. Submitting invalid credentials shows backend error and refreshes captcha.
5. Register tab rejects username values containing `_` or Chinese characters.
6. Register tab rejects mismatched passwords.
7. Register email code button rejects invalid email.
8. Forgot-password tab validates email, email code, and matching passwords.
9. Forgot-password submit shows `找回密码功能暂未开放`.

Expected: all checks pass.

- [ ] **Step 5: Manual AccountPage checks with backend available**

Check these flows against the running RuoYi-Vue-Plus backend:

1. Login with valid username, password, captcha.
2. App switches to AccountPage.
3. AccountPage shows avatar placeholder or avatar image, nickname, username, email, user ID, tenant ID.
4. Points card shows points or `暂无积分数据或加载失败` without breaking profile display.
5. Password form rejects mismatched new passwords.
6. Password form shows backend error for wrong old password.
7. Logout returns to AuthPage.

Expected: all checks pass.

- [ ] **Step 6: Run code review agents**

Dispatch these agents after implementation:

- `everything-claude-code:typescript-reviewer` for `src/**/*.ts` and `src/**/*.tsx`.
- `everything-claude-code:rust-reviewer` for `src-tauri/src/**/*.rs`.
- `everything-claude-code:security-reviewer` for token storage, encryption, and HTTP request handling.

Expected: no CRITICAL or HIGH findings remain. Fix MEDIUM findings that are directly related to this feature.

---

## Self-Review Notes

Spec coverage:

- AuthPage login/register/forgot-password: covered by Task 5.
- Rust-only API integration: covered by Task 2 and Task 3.
- Fixed `tenantId=000000`, fixed `clientId`, password grant: covered by `auth_api.rs` constants and request bodies.
- Graph captcha: covered by `get_captcha()` and AuthPage login form.
- Email code button with countdown: covered by Task 5.
- Account profile, avatar, username, email: covered by Task 6.
- Points display: covered by Task 6.
- Logged-in password update: covered by Task 2 and Task 6.
- Forgot-password form without real reset: covered by Task 5.
- Dev unencrypted / prod encrypted: covered by `request_payload()` in Task 2.
- Verification: covered by Task 8.

Placeholder scan: no `TBD`, `TODO`, or open-ended implementation steps are intentionally left in this plan.

Type consistency: Rust command names match TypeScript wrapper names; TypeScript wrapper parameter names match Tauri command argument names; React props match `App.tsx` usage.
