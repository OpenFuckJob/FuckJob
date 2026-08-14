use crate::{
    agent,
    agent::tasks::GreetTask,
    config::{AppRuntimeConfig, GreetConfig, ReplayResourceType, ReplyResource},
    logger,
    rpa::common::RpaJob,
    rpa::conversation::{self, GreetAction, SendVerdict},
};

/// 把配置的显式发送序列转换为最终资源列表。
///
/// 禁用项和空内容不会发送。LLM 那一条**生成失败**时只跳过它、不影响后续固定内容——
/// 那属于服务不可用，固定的自我介绍照发是合理的。
/// 但模型判断「不该投」是另一回事，那要整轮取消，由调用方在这之前拦掉。
fn compose_greet_resources(greet: &GreetConfig, generated: Option<String>) -> Vec<ReplyResource> {
    let generated = generated.filter(|text| !text.trim().is_empty());

    greet
        .default_template
        .iter()
        .filter(|resource| resource.enabled)
        .filter_map(|resource| {
            let content = if resource.resource_type == ReplayResourceType::LLM {
                generated.clone()?
            } else {
                resource.content.clone()
            };
            (!content.trim().is_empty()).then_some(ReplyResource {
                resource_type: resource.resource_type.clone(),
                content,
            })
        })
        .collect()
}

/// 组装一个岗位的打招呼发送序列。
///
/// 返回 [`SendVerdict::Hold`] 表示这个岗位整轮都不该发——可能是模型判断不该投，
/// 也可能是内容没通过发送前体检。调用方必须尊重它：不能"至少把固定内容发出去"。
pub async fn build_greet_resources(
    config: &AppRuntimeConfig,
    job: &RpaJob,
) -> Result<SendVerdict, anyhow::Error> {
    let mut generated = None;

    if config.greet_config.llm_resource_ready() {
        match agent::run(&GreetTask::new(config, job), config).await {
            Ok(outcome) => {
                let decision = outcome.output;
                if decision.action == GreetAction::Skip {
                    // 模型有正规渠道说「不该投」，这条路径存在的意义就是让这个结论
                    // 变成一个动作，而不是被写进正文发给对方
                    return Ok(SendVerdict::Hold(format!(
                        "模型判断该岗位不适合投递（把握 {} 分）：{}",
                        decision.confidence, decision.reason
                    )));
                }
                generated = Some(decision.greeting);
            }
            // 生成失败属于服务不可用，不代表这个岗位不该投，固定内容仍然可以发
            Err(error) => {
                logger::warning(format!("LLM 打招呼生成失败，已跳过该条: {}", error))?;
            }
        }
    }

    let resources = compose_greet_resources(&config.greet_config, generated);
    if resources.is_empty() {
        return Err(anyhow::anyhow!(
            "打招呼发送序列没有可发送内容，请至少启用并配置一条有效内容"
        ));
    }

    // 和自动回复共用同一道门：任何一条不合格就整轮不发
    Ok(conversation::vet_outbound(
        resources,
        config.replay_config.max_reply_chars,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GreetResource;

    fn resource(resource_type: ReplayResourceType, content: &str, enabled: bool) -> GreetResource {
        GreetResource {
            enabled,
            resource_type,
            content: content.to_string(),
        }
    }

    fn greet(resources: Vec<GreetResource>) -> GreetConfig {
        GreetConfig {
            enable_llm: true,
            reply_prompt: Some("生成打招呼内容".to_string()),
            default_template: resources,
        }
    }

    #[test]
    fn explicit_sequence_keeps_enabled_items_in_order() {
        let greet = greet(vec![
            resource(ReplayResourceType::Text, "第一条", true),
            resource(ReplayResourceType::LLM, "", true),
            resource(ReplayResourceType::Text, "第三条", true),
        ]);

        let resources = compose_greet_resources(&greet, Some("模型内容".to_string()));

        assert_eq!(resources.len(), 3);
        assert_eq!(resources[0].content, "第一条");
        assert_eq!(resources[1].content, "模型内容");
        assert_eq!(resources[2].content, "第三条");
    }

    #[test]
    fn disabled_and_blank_items_are_not_sent() {
        let greet = greet(vec![
            resource(ReplayResourceType::Text, "停用内容", false),
            resource(ReplayResourceType::LLM, "", false),
            resource(ReplayResourceType::Text, "   ", true),
            resource(ReplayResourceType::Text, "有效内容", true),
        ]);

        assert!(!greet.llm_resource_ready());
        assert_eq!(compose_greet_resources(&greet, None).len(), 1);
        assert_eq!(compose_greet_resources(&greet, None)[0].content, "有效内容");
    }

    /// 模型服务挂掉不代表这个岗位不该投，固定的自我介绍照发
    #[test]
    fn llm_failure_skips_only_llm_item() {
        let greet = greet(vec![
            resource(ReplayResourceType::Text, "开场", true),
            resource(ReplayResourceType::LLM, "", true),
            resource(ReplayResourceType::Text, "兜底", true),
        ]);

        let resources = compose_greet_resources(&greet, None);

        assert_eq!(resources.len(), 2);
        assert_eq!(resources[0].content, "开场");
        assert_eq!(resources[1].content, "兜底");
    }

    #[test]
    fn llm_is_not_implicitly_inserted_without_a_slot() {
        let greet = greet(vec![resource(ReplayResourceType::Text, "固定内容", true)]);

        assert!(!greet.llm_resource_ready());
        let resources = compose_greet_resources(&greet, Some("不应发送".to_string()));
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].content, "固定内容");
    }

    /// 实测事故：模型把婉拒写进开场白，那条被发了出去，
    /// 紧跟着的简历图片也照发。整轮必须一起取消
    #[test]
    fn a_declining_greeting_holds_the_whole_sequence_including_the_image() {
        let resources = vec![
            ReplyResource {
                resource_type: ReplayResourceType::LLM,
                content: "您好，注意到您是猎头顾问，我暂时不考虑猎头渠道，感谢理解。".to_string(),
            },
            ReplyResource {
                resource_type: ReplayResourceType::Image,
                content: "C:/resume.png".to_string(),
            },
        ];

        match conversation::vet_outbound(resources, 200) {
            SendVerdict::Hold(reason) => assert!(reason.contains("不考虑")),
            other => panic!("必须整轮拦下，实际：{other:?}"),
        }
    }
}
