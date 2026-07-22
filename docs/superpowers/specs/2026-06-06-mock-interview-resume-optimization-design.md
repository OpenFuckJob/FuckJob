# 模拟面试与简历优化闭环功能设计规格说明书 (Spec)

## 1. 业务背景与用户价值

用户在配置中心导入简历，或直接在简历工作台（Resume Studio）编写简历后，往往面临如下痛点：
1. **内容空泛**：项目经历过于简单，缺乏核心技术细节及量化成果支撑。
2. **重点不突出**：不知道面试官会针对当前简历提出哪些具有杀伤力的问题，无法做针对性准备。
3. **改写困难**：知道自己做过什么，但无法用专业（如 STAR 原则）的简历语言将其重构。

通过引入**“简历智能诊断提问 -> 结合回答自动重构 -> 一键应用并存档”**的闭环，让用户在与 AI 面试官的一问一答中，轻松将琐碎的个人回答转化为高质量、高可信度、有数据支撑的简历亮点。

---

## 2. 架构设计与模块分工

本项目为 Tauri + React + Vite 架构。本功能的具体实现分为以下几个部分：

```
[前端: React + Antd]                         [后端: Tauri Core (Rust)]
┌────────────────────────┐                  ┌─────────────────────────┐
│  ResumeOptimizerPage   │                  │  Tauri Command Handlers │
│   (UI 挂载入口按钮)     │                  │                         │
└───────────┬────────────┘                  │  1. predict_resume_      │
            │ 打开 Drawer                   │     questions            │
            ▼                               │  2. optimize_resume_     │
┌────────────────────────┐  调用 IPC 命令   │     with_answer          │
│  MockInterviewDrawer   ├─────────────────►│                         │
│  (预测/答题/Diff对比)   │◄─────────────────┤  (通过 reqwest 请求)     │
└───────────┬────────────┘  返回大模型文本   └────────────┬────────────┘
            │                                            │ 
            │ 确认修改                                   │ 发送请求
            ▼                                            ▼
┌────────────────────────┐                  ┌─────────────────────────┐
│  replaceSectionContent │                  │  Server API             │
│  (更新 React 状态并保存) │                  │  (/rpa/generate/text)   │
└────────────────────────┘                  └─────────────────────────┘
```

### 2.1 模块分工
1. **前端交互 (`src/view/resume-optimizer/`)**：
   - 挂载“模拟面试优化”入口按钮。
   - 实现交互抽屉 `MockInterviewDrawer`。
   - 调用 Tauri IPC 接口获取预测题和优化结果。
   - 渲染 Diff 对比和替换 Markdown。
2. **后端服务 (`src-tauri/src/command/llm.rs` & `server_api/`)**：
   - 封装 `predict_resume_questions` 预测逻辑，向 LLM 请求简历薄弱点追问。
   - 封装 `optimize_resume_with_answer` 优化重构逻辑，传递原内容、提问和回答，让大模型生成 STAR 格式 Markdown 段落。
   - 依赖并复用 `server_api::generate::generate_text` 请求服务器 LLM 网关。

---

## 3. 详细设计规格 (Detailed Spec)

### 3.1 数据结构定义 (Types)

#### 前端 TypeScript (`src/types/analysis.ts` 扩展)
```typescript
export interface PredictedQuestion {
  id: number;
  question: string;         // 追问的面试题
  intent: string;           // 面试官的提问意图
  target_section: string;   // 建议优化的简历章节名称（如：项目经历、工作经历）
}

export interface OptimizeWithAnswerRequest {
  resume_content: string;   // 完整简历 Markdown
  question: string;         // 提出的面试问题
  user_answer: string;      // 用户输入的回答
  section_title: string;    // 需要更新的章节标题 (例如 "项目经历")
}
```

#### 后端 Rust Struct (`src-tauri/src/command/llm.rs`)
```rust
#[derive(Debug, serde::Deserialize)]
pub struct OptimizeWithAnswerRequest {
    pub resume_content: String,
    pub question: String,
    pub user_answer: String,
    pub section_title: String,
}

#[derive(Debug, serde::Serialize)]
pub struct ResumeLlmResult {
    pub success: bool,
    pub data: String,
}
```

### 3.2 大模型 Prompt 详细设计

#### 3.2.1 简历缺陷提问预测 (`predict_resume_questions`)
- **角色**：挑剔的大厂面试官。
- **任务**：指出简历中空泛、缺乏量化指标、技术表述模糊的 5 个地方，并进行深度追问。
- **输出约束**：纯 JSON 格式，严格遵循预设的结构体定义，严禁有多余解释。

**Prompt 模板**：
```
你是一位挑剔且经验丰富的技术面试官。请仔细阅读以下候选人的 Markdown 简历：
---
{resume_content}
---
请找出简历中不详实、缺乏量化指标（如QPS、性能提升百分比、业务成效）、或者技术方案可能存在漏洞的 5 个薄弱点。
针对这 5 个薄弱点，提出 5 个在真实面试中面试官最可能追问的深度专业问题，并说明你的提问意图（想考察候选人什么底层能力）。

输出约束（极其重要）：
只输出一个合法的 JSON 数组，不要包含任何 Markdown 代码块标记（如 ```json），不要有任何前言、后记或解释。

JSON 数组格式如下：
[
  {
    "id": 1,
    "question": "具体追问的问题，如：你提到在网关层做限流，能详细说说令牌桶算法和漏桶算法的区别，以及你们为什么选择前者吗？",
    "intent": "考察对高并发限流方案的底层掌握程度及技术选型思考",
    "target_section": "项目经历"
  }
]
```

#### 3.2.2 结合回答优化简历 (`optimize_resume_with_answer`)
- **角色**：资深简历精修专家。
- **任务**：提取用户回答中的事实、数据和方案，按照 STAR 原则融进原章节中，重新润色段落。
- **输出约束**：仅输出经过修改优化后的整个 Markdown 二级章节，不要生成前言和致谢。

**Prompt 模板**：
```
你是一位简历打磨专家。候选人针对简历中的某项缺陷回答了面试官的追问。请将他回答中包含的有效信息（技术细节、行动步骤、可量化的数据结果）重构融进简历对应章节中。

原简历内容：
{resume_content}

面试提问：
{question}

候选人的回答：
{user_answer}

关联优化章节：
{section_title}

优化及重构要求：
1. 提取回答中的闪光点，将其提炼为符合“STAR原则”（情境-任务-行动-结果）的描述。
2. 保持简历专业、简洁的学术风格，用词要精确（如“负责、主导、重构、优化”）。
3. 只输出优化重构后的【整个章节】（必须包含原标题，如 ## {section_title}）的 Markdown 文本，原简历其他章节无需输出。
4. 禁止输出任何解释、引导语、注释或包裹 Markdown 块标记。

输出格式示例：
## {section_title}
- 优化后的内容1...
- 优化后的内容2...
```

---

## 4. 界面交互与操作流设计

### 4.1 入口与 Tab 切换
1. 在 `src/App.tsx` 中解除对 `resume-optimizer` 标签的注释，使侧边栏重新显示**「简历优化」**菜单。
2. 页面进入「简历工作台」后，页面顶部工具栏新增**「模拟面试优化」**按钮。

### 4.2 模拟面试优化抽屉 (`MockInterviewDrawer`) 交互流程
1. 点击「模拟面试优化」按钮后，从右侧滑出 Drawer。
2. 初始状态：显示功能说明和“**开始分析我的简历**”的显著按钮。
3. 点击分析：
   - 页面进入 `loading` 状态。
   - 调用后端 `predict_resume_questions` 获取 5 个深度追问问题。
   - 在 Drawer 上半部分以列表（`List`）形式渲染这 5 个问题。
4. 选择问题作答：
   - 用户点击某问题旁的“模拟回答”按钮。
   - Drawer 下半部分展开**回答输入框**（`Input.TextArea`）和面试官提问意图展示。
   - 用户在此输入有关该问题的补充和答复。
5. 优化对比：
   - 用户输入完毕后，点击“结合回答优化简历”。
   - 前端向 Rust 发送 `optimize_resume_with_answer` 命令。
   - 返回优化后的 Markdown 模块。
   - 渲染一块背景为绿色的对比预览框，展示优化后的 Markdown 段落。
6. 应用并保存：
   - 用户查看优化内容无误后，点击“采纳并更新”。
   - 前端触发 `handleApplyMockOptimization`，定位并替换主编辑器中的对应章节。
   - 状态自动同步并调用 `saveUserResumes` 写入本地 JSON，右下角触发 `message.success` 提示。

---

## 5. 测试用例与自检

| 编号 | 测试场景 | 预期结果 |
| :--- | :--- | :--- |
| TC-01 | 无简历时点击优化 | 按钮应置灰或提示“请先输入/导入简历内容” |
| TC-02 | 简历导入后触发预测 | 调用 API，展示包含问题、意图、关联章节的 5 个列表项 |
| TC-03 | 用户输入空回答提交 | 提示“请输入您的真实回答”，不发送网络请求 |
| TC-04 | 提交有效回答并采纳 | 简历的对应章节被替换更新，且 Markdown 语法无破损 |
| TC-05 | 重新进入页面验证保存 | 刷新或重新选择当前简历，之前优化的内容能正确加载（代表成功存入本地 json） |
