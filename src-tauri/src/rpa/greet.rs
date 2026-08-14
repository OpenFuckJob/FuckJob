use crate::{
    config::{AppRuntimeConfig, GreetConfig, ReplayResourceType, ReplyResource},
    llm::generate_greet_text,
    logger,
    rpa::common::RpaJob,
};

/// 把配置的显式发送序列转换为最终资源列表。
/// 禁用项和空内容不会发送；LLM 生成不可用时仅跳过 LLM 项，不影响后续固定内容。
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

pub async fn build_greet_resources(
    config: &AppRuntimeConfig,
    job: &RpaJob,
) -> Result<Vec<ReplyResource>, anyhow::Error> {
    let generated = if config.greet_config.llm_resource_ready() {
        match generate_greet_text(config.clone(), job).await {
            Ok(result) if result.success && !result.data.trim().is_empty() => Some(result.data),
            Ok(_) => {
                logger::warning("LLM 未生成打招呼内容，已跳过该条")?;
                None
            }
            Err(error) => {
                logger::warning(format!("LLM 打招呼生成失败，已跳过该条: {}", error))?;
                None
            }
        }
    } else {
        None
    };

    let resources = compose_greet_resources(&config.greet_config, generated);
    if resources.is_empty() {
        return Err(anyhow::anyhow!(
            "打招呼发送序列没有可发送内容，请至少启用并配置一条有效内容"
        ));
    }
    Ok(resources)
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
}
