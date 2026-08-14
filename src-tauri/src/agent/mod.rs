//! 统一的大模型调用层。
//!
//! 所有非流式的模型用途都走这里：任务只描述「给什么上下文、要什么结果、什么算合格」，
//! 调用、重试、降级、输出净化、校验返工由 [`run`] 统一负责。
//!
//! 边界：模拟面试那类需要边生成边展示的流式调用**不并入**——
//! 它的增量已经推送到界面，重试会让用户看到重复内容。

pub mod output;
pub mod run;
pub mod tasks;

pub use run::{run, AgentOutcome, AgentRunner, AgentStop, AgentTask};
