//! 统一的大模型调用层。
//!
//! 所有模型用途都走这里：任务只描述「给什么上下文、要什么结果、什么算合格」，
//! 调用、重试、降级、输出净化、校验返工由 [`run`] 统一负责。
//!
//! 流式也在其中。需要边生成边展示时用 [`AgentRunner::run_streaming`]，
//! 循环会自动切换到「单轮 + 只在尚未吐字时才降级」的模式——
//! 增量一旦推到界面上，重发就会让用户看到重复内容。把这条约束做成循环的一种模式，
//! 而不是让流式调用另起炉灶，才不会有用途长期游离在统一的取消、日志、净化之外。
//!
//! 唯一的例外是 provider 连接测试（`command::llm_provider`）：它验证的是单个服务
//! 通不通，走降级链会让备用服务的成功掩盖主用服务的故障。理由写在那边的代码里。
//!
//! 内置提示词统一放在 [`prompts`]，用户可编辑的打招呼与回复提示词仍在配置文件里。

pub mod output;
pub mod prompts;
pub mod run;
pub mod tasks;

pub use run::{run, AgentOutcome, AgentRunner, AgentStop, AgentTask};
