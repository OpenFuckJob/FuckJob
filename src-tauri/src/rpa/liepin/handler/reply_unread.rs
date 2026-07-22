use std::time::Duration;

use anyhow::Context;
use regex::Regex;
use rust_drission::{utils::sleep_random_ms, Element, Page};

use crate::{
    browser,
    config::{AppRuntimeConfig, ReplayResourceType, ReplyResource, ReplyTemplate},
    logger,
    rpa::{
        common::ChatMessage,
        liepin::{handler::position_say_hello::send_resources, LIEPIN_CHAT_URL},
        run_flow::is_job_task_stop_requested,
    },
};

pub async fn reply_unread(config: &AppRuntimeConfig) -> Result<Vec<ChatMessage>, anyhow::Error> {
    let config = config.clone();

    browser::with_new_tab(|page| {
        Box::pin(async move {
            logger::info("正在打开猎聘沟通页面")?;
            page.get(LIEPIN_CHAT_URL)?;
            sleep_random_ms(1200, 2000);

            click_unread_filter(page)?;
            let replay_config = config.replay_config.clone();

            loop {
                if is_job_task_stop_requested() {
                    logger::info("猎聘沟通任务已结束")?;
                    return Ok(Vec::new());
                }

                let chats = collect_chat_cards(page)?;
                if chats.is_empty() {
                    logger::info("猎聘没有未读消息")?;
                    return Ok(Vec::new());
                }

                logger::info(format!("猎聘当前未读会话数: {}", chats.len()))?;
                for (index, chat) in chats.iter().enumerate() {
                    if is_job_task_stop_requested() {
                        logger::info("猎聘沟通任务已结束")?;
                        return Ok(Vec::new());
                    }

                    chat.click()?;
                    sleep_random_ms(800, 1200);
                    click_resume_action(page)?;

                    let chat_messages = collect_chat_messages(page)?;
                    let latest = chat_messages
                        .last()
                        .map(|m| m.text.as_str())
                        .unwrap_or("(空)");
                    logger::info(format!(
                        "处理猎聘第 {}/{} 个会话，最近消息: {}",
                        index + 1,
                        chats.len(),
                        truncate_str(latest, 30)
                    ))?;

                    if let Some(resources) =
                        resolve_reply_resources(&config, &replay_config.templates, &chat_messages)
                            .await?
                    {
                        send_resources(page, resources)?;
                        logger::info("猎聘回复消息已发送")?;
                    } else {
                        logger::info("猎聘未匹配到回复模板，跳过")?;
                    }

                    sleep_random_ms(2500, 4500);
                }

                if !scroll_chat_list(page)? {
                    logger::info("猎聘未读消息已处理完成")?;
                    return Ok(Vec::new());
                }
            }
        })
    })
    .await
}

fn click_unread_filter(page: &Page) -> Result<(), anyhow::Error> {
    for selector in [
        "[class*='unread']",
        "button[class*='unread']",
        "span[class*='unread']",
        ".message-filter-unread",
    ] {
        if let Some(ele) = page.ele(selector)? {
            ele.click()?;
            sleep_random_ms(500, 800);
            return Ok(());
        }
    }
    Ok(())
}

fn collect_chat_cards(page: &Page) -> Result<Vec<Element>, anyhow::Error> {
    for selector in [
        ".chat-list-item",
        ".message-list-item",
        "[class*='chat-item']",
        "[class*='message-item']",
        "li[class*='session']",
    ] {
        let cards = page.eles(selector)?;
        if !cards.is_empty() {
            return Ok(cards);
        }
    }
    Ok(Vec::new())
}

fn click_resume_action(page: &Page) -> Result<(), anyhow::Error> {
    for selector in [
        ".send-resume",
        ".resume-btn",
        "button[class*='resume']",
        "a[class*='resume']",
        "button[class*='confirm']",
    ] {
        if let Some(ele) = page.ele(selector)? {
            ele.click()?;
            sleep_random_ms(500, 800);
            click_confirm_if_present(page)?;
            return Ok(());
        }
    }
    Ok(())
}

fn click_confirm_if_present(page: &Page) -> Result<(), anyhow::Error> {
    for selector in [
        ".ant-modal-confirm-btns .ant-btn-primary",
        ".modal .confirm",
        "button[class*='confirm']",
        "button[class*='sure']",
    ] {
        if let Some(ele) = page.ele(selector)? {
            ele.click()?;
            sleep_random_ms(500, 800);
            return Ok(());
        }
    }
    Ok(())
}

fn collect_chat_messages(page: &Page) -> Result<Vec<ChatMessage>, anyhow::Error> {
    let value = page
        .run_js_await(
            r#"
            (() => {
                const selectors = [
                    ".message-content",
                    ".chat-message",
                    "[class*='message-content']",
                    "[class*='bubble']",
                    "[class*='chat-msg']"
                ];
                const nodes = selectors.flatMap((selector) => Array.from(document.querySelectorAll(selector)));
                const seen = new Set();
                return nodes.map((node, index) => {
                    const text = (node.innerText || node.textContent || "").trim();
                    if (!text || seen.has(text + index)) return null;
                    seen.add(text + index);
                    const className = node.className || "";
                    return {
                        mid: index,
                        received: !/self|mine|right|me/.test(String(className)),
                        text,
                        time: Date.now(),
                        from_name: ""
                    };
                }).filter(Boolean);
            })()
            "#,
        )
        .context("读取猎聘聊天记录失败")?;

    let raw = value.get("value").cloned().unwrap_or(value);
    let messages =
        serde_json::from_value::<Vec<ChatMessage>>(raw).context("解析猎聘聊天记录失败")?;

    Ok(messages)
}

async fn resolve_reply_resources(
    config: &AppRuntimeConfig,
    templates: &[ReplyTemplate],
    chat_messages: &[ChatMessage],
) -> Result<Option<Vec<ReplyResource>>, anyhow::Error> {
    if config.replay_config.enable_llm {
        if chat_messages.is_empty() {
            return Ok(None);
        }

        match crate::llm::generate_replay_text(String::new(), config, chat_messages).await {
            Ok(result) if result.success && !result.data.trim().is_empty() => {
                return Ok(Some(vec![ReplyResource {
                    resource_type: ReplayResourceType::Text,
                    content: result.data,
                }]));
            }
            Ok(_) => logger::warning("LLM 未生成可发送内容，改用显式文本模板")?,
            Err(error) => logger::warning(format!("LLM 生成失败，改用显式文本模板: {}", error))?,
        }
    }

    if let Some(template) = match_reply_template(templates, chat_messages) {
        let resources: Vec<ReplyResource> = template
            .content
            .iter()
            .filter(|r| !r.content.trim().is_empty())
            .cloned()
            .collect();
        if !resources.is_empty() {
            return Ok(Some(resources));
        }
    }

    Ok(None)
}

fn match_reply_template<'a>(
    templates: &'a [ReplyTemplate],
    chat_messages: &[ChatMessage],
) -> Option<&'a ReplyTemplate> {
    templates.iter().find(|template| {
        Regex::new(&template.regex_rule.pattern)
            .map(|regex| {
                regex.is_match(&latest_chat_history(
                    chat_messages,
                    template.regex_rule.limit,
                ))
            })
            .unwrap_or(false)
    })
}

fn latest_chat_history(chat_messages: &[ChatMessage], limit: i32) -> String {
    let limit = usize::try_from(limit.max(1)).unwrap_or(1);
    let start = chat_messages.len().saturating_sub(limit);
    chat_messages[start..]
        .iter()
        .map(|message| message.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn scroll_chat_list(page: &Page) -> Result<bool, anyhow::Error> {
    let before = page.run_js_await(
        r#"
        (() => {
            const el = document.querySelector("[class*='list']") || document.scrollingElement;
            return el ? el.scrollTop : 0;
        })()
        "#,
    )?;
    page.run_js_await(
        r#"
        (() => {
            const el = document.querySelector("[class*='list']") || document.scrollingElement;
            if (!el) return 0;
            el.scrollTop += 500;
            return el.scrollTop;
        })()
        "#,
    )?;
    std::thread::sleep(Duration::from_millis(600));
    let after = page.run_js_await(
        r#"
        (() => {
            const el = document.querySelector("[class*='list']") || document.scrollingElement;
            return el ? el.scrollTop : 0;
        })()
        "#,
    )?;
    Ok(before != after)
}

fn truncate_str(s: &str, max_len: usize) -> &str {
    if s.chars().count() <= max_len {
        s
    } else {
        let end = s
            .char_indices()
            .nth(max_len)
            .map(|(i, _)| i)
            .unwrap_or(s.len());
        &s[..end]
    }
}
