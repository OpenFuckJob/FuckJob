//! Opt-in live acceptance through the same task manager used by the desktop UI.
//! cargo run --example job51_acceptance -- inspect|login|single|periodic
use anyhow::{bail, Context, Result};
use offer_flow_lib::{
    browser, config,
    dao::{self, job_detail_dao, model::JobProfileSnapshot, profile_snapshot_dao},
    logger,
    rpa::{
        run_flow::{self, FlowMode, PlatformKind},
        schedule::PeriodicPlan,
    },
    task::{JobTaskProfile, JobTaskState, JOB_TASK_MANAGER},
};
use serde_json::json;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use tauri::Manager;

fn main() -> Result<()> {
    let mode = std::env::args().nth(1).unwrap_or("inspect".into());
    if let Some(ids) = std::env::args().nth(2).filter(|_|mode!="prepare-public") {
        offer_flow_lib::rpa::job51::restrict_acceptance_jobs(ids.split(',').map(str::to_owned).collect())?;
        println!("ACCEPTANCE_JOB_IDS {ids}");
    }
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    let app = tauri::Builder::default().build(context)?;
    let handle = app.handle().clone();
    logger::init(&handle)?;
    browser::init_app_handle(handle.clone())?;
    let base =
        config::load_app_config_inner(handle.clone()).map_err(|error| anyhow::anyhow!(error))?;
    let resolved = config::resolve_job_profile(&base, None).map_err(anyhow::Error::msg)?;
    let mut runtime = resolved.config;
    let output_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/job51-acceptance");
    std::fs::create_dir_all(&output_dir)?;
    if mode == "inspect" {
        println!(
            "{}",
            json!({"profile":resolved.profile_name,"llm_active":runtime.llm_active(),
            "resume_present":runtime.resume_config.resume_content.as_deref().is_some_and(|text|!text.trim().is_empty()),
            "query":runtime.job_filter_config.query,"city":runtime.job_filter_config.city,
            "filters":runtime.platform_filter_config.job51,"dry_run":runtime.replay_config.dry_run})
        );
        return Ok(());
    }
    if mode == "enable-three-platforms" {
        let mut updated=base;
        updated.browser_config.max_parallel_tasks=3;
        config::save_app_config_inner(handle.clone(),updated).map_err(|error|anyhow::anyhow!(error))?;
        println!("MAX_PARALLEL_TASKS_SET 3");
        return Ok(());
    }
    if mode == "data" {
        dao::init(&handle.path().app_data_dir()?)?;
        let list=offer_flow_lib::command::job::job_list_with_status();
        let overview=offer_flow_lib::command::job::job_search_overview(handle.clone(),Some(0));
        if !list.success || !overview.success { bail!("岗位管理或求职数据查询失败"); }
        let ids=std::env::args().nth(2).context("job IDs required")?;
        let list=list.data.context("岗位列表为空")?;
        let rows=list.iter().filter(|job|ids.split(',').any(|id|job.job.id==format!("51job:{id}"))).collect::<Vec<_>>();
        println!("DATA_VERIFICATION {}",serde_json::to_string(&json!({"jobs":rows,"overview":overview.data}))?);
        return Ok(());
    }
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    if mode == "navigation" {
        rt.block_on(browser::with_browser(|connection| Box::pin(async move {
            for page in connection.tabs()? {
                let url=page.url()?;
                if tauri::Url::parse(&url).ok().and_then(|url|url.host_str().map(str::to_owned)).as_deref()!=Some("jobs.51job.com") { continue; }
                println!("JD_NAVIGATION {}",page.run_js_await("({title:document.title,path:location.pathname,referrer:document.referrer,hasOpener:!!window.opener})")?);
            }
            Ok(())
        })))?;
        return Ok(());
    }
    if mode == "inspect-slider" || mode == "try-slider" || mode == "refresh-slider" {
        let id=std::env::args().nth(2).context("reviewed job ID required")?;
        let refresh=mode=="refresh-slider";
        let attempt=mode=="try-slider" || refresh;
        let show=attempt || std::env::args().nth(3).as_deref()==Some("show");
        let request=std::env::args().nth(3).filter(|value|value!="show");
        let output_dir=output_dir.clone();
        // Diagnostics must not register stealth scripts on a pre-existing verification page.
        let record:serde_json::Value=serde_json::from_slice(&std::fs::read(handle.path().app_data_dir()?.join("rpa-browser.json"))?)?;
        let port=record["port"].as_u64().context("managed Chrome port missing")?;
        let connection=rust_drission::ChromiumPage::connect(&format!("127.0.0.1:{port}"))?;
        rt.block_on(async move {
            for page in connection.tabs()? {
                let url=tauri::Url::parse(&page.url()?)?;
                if url.host_str()!=Some("jobs.51job.com") || !url.path().ends_with(&format!("/{id}.html")) { continue; }
                if request.as_ref().is_some_and(|expected|!url.query_pairs().any(|(name,value)|name=="req" && value==expected.as_str())) { continue; }
                println!("VERIFICATION_WINDOW {:?}",page.run_cdp("Browser.getWindowForTarget",Some(json!({"targetId":page.tab_id()}))));
                println!("VERIFICATION_VIEWPORT {}",page.run_js_await("({visibility:document.visibilityState,focus:document.hasFocus(),screenX,screenY,outerWidth,outerHeight,innerWidth,innerHeight,devicePixelRatio})")?);
                if attempt {
                    let window=page.run_cdp("Browser.getWindowForTarget",Some(json!({"targetId":page.tab_id()})))?;
                    if window["bounds"]["windowState"]=="minimized" {
                        page.run_cdp("Browser.setWindowBounds",Some(json!({"windowId":window["windowId"],"bounds":{"windowState":"normal"}})))?;
                    }
                    page.run_cdp("Page.bringToFront",None)?;
                    for _ in 0..20 {
                        let visible=page.run_js_await("document.visibilityState==='visible'")?;
                        if visible.get("value").unwrap_or(&visible)==true { break; }
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                    let visible=page.run_js_await("document.visibilityState==='visible'")?;
                    if visible.get("value").unwrap_or(&visible)!=true { bail!("Chrome窗口仍不可见，未执行拖动"); }
                    println!("ACTIVE_VERIFICATION_VIEWPORT {}",page.run_js_await("({visibility:document.visibilityState,focus:document.hasFocus(),screenX,screenY,outerWidth,outerHeight})")?);
                    if refresh { page.refresh()?; }
                    let expression="(()=>{const s=document.getElementById('aliyunCaptcha-sliding-slider');const b=document.getElementById('aliyunCaptcha-sliding-body');if(document.title!=='滑动验证页面'||!s||!b)return null;const r=s.getBoundingClientRect(),q=b.getBoundingClientRect();if(r.width<=0||r.height<=0||q.width<=r.width)return null;return {x:r.x+r.width/2,y:r.y+r.height/2,end:q.right-r.width/2};})()";
                    let mut geometry=serde_json::Value::Null;
                    for _ in 0..40 {
                        geometry=page.run_js_await(expression)?;
                        if geometry.get("value").unwrap_or(&geometry).get("x").is_some() { break; }
                        tokio::time::sleep(Duration::from_millis(250)).await;
                    }
                    let geometry=geometry.get("value").unwrap_or(&geometry);
                    let x=geometry["x"].as_f64().context("current slider is absent")?;
                    let y=geometry["y"].as_f64().context("current slider is absent")?;
                    let end=geometry["end"].as_f64().context("current slider rail is absent")?;
                    page.run_cdp("Input.dispatchMouseEvent",Some(json!({"type":"mouseMoved","x":x,"y":y})))?;
                    page.run_cdp("Input.dispatchMouseEvent",Some(json!({"type":"mousePressed","x":x,"y":y,"button":"left","buttons":1,"clickCount":1})))?;
                    let dragged=async {
                        for step in 1..=20 {
                            page.run_cdp("Input.dispatchMouseEvent",Some(json!({"type":"mouseMoved","x":x+(end-x)*step as f64/20.0,"y":y,"button":"left","buttons":1})))?;
                            tokio::time::sleep(Duration::from_millis(50)).await;
                        }
                        Ok::<_,anyhow::Error>(())
                    }.await;
                    let released=page.run_cdp("Input.dispatchMouseEvent",Some(json!({"type":"mouseReleased","x":end,"y":y,"button":"left","buttons":0,"clickCount":1})));
                    dragged?;
                    released?;
                    tokio::time::sleep(Duration::from_secs(3)).await;
                }
                if show {
                    page.run_cdp("Page.bringToFront",None)?;
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    let capture_page=page.clone();
                    let (sender,receiver)=std::sync::mpsc::channel();
                    std::thread::spawn(move || { let _=sender.send(capture_page.run_cdp("Page.captureScreenshot",Some(json!({"format":"png","fromSurface":true,"captureBeyondViewport":false})))); });
                    if let Ok(Ok(shot))=receiver.recv_timeout(Duration::from_secs(5)) {
                        if let Some(data)=shot.get("data").and_then(serde_json::Value::as_str) {
                            use base64::Engine;
                            let path=output_dir.join(format!("slider-{id}.png"));
                            std::fs::write(&path,base64::engine::general_purpose::STANDARD.decode(data)?)?;
                            println!("SLIDER_SCREENSHOT {}",path.display());
                        }
                    } else { bail!("验证码截图5秒内未就绪，已停止只读诊断"); }
                }
                println!("SLIDER_UI {}",page.run_js_await("({title:document.title,prompt:(document.body?.innerText||'').slice(0,600),controls:Array.from(document.querySelectorAll('iframe,button,input,[role=button],[id*=aptcha],[class*=aptcha],[class*=slider],.waf-nc-title,.nc_scale,.nc-lang-cnt,.btn_slide,[id$=_n1z],[id$=_wrapper]')).slice(0,50).map(e=>({tag:e.tagName,id:e.id,cls:String(e.className),text:(e.innerText||'').slice(0,120),rect:(()=>{const r=e.getBoundingClientRect();return {x:r.x,y:r.y,width:r.width,height:r.height}})()}))})")?);
                if attempt {
                    let result=page.run_js_await("({title:document.title,host:location.hostname,path:location.pathname,detailLoaded:!!document.querySelector('.job_msg')?.textContent?.trim()})")?;
                    let result=result.get("value").unwrap_or(&result);
                    if result["title"]=="滑动验证页面" || result["host"]!="jobs.51job.com"
                        || result["detailLoaded"]!=true || !result["path"].as_str().unwrap_or_default().ends_with(&format!("/{id}.html")) {
                        bail!("网站尚未确认验证通过，正常JD未加载；本次尝试结束");
                    }
                    println!("VERIFICATION_PASSED {id}");
                }
                return Ok(());
            }
            bail!("specified current verification page is absent");
        })?;
        return Ok(());
    }
    if mode == "normal-navigation" {
        let id=std::env::args().nth(2).context("one reviewed job ID required")?;
        if !id.bytes().all(|byte|byte.is_ascii_digit()) { bail!("one numeric job ID required"); }
        let query=std::env::args().nth(3).unwrap_or_else(||"java".into());
        if !runtime.job_filter_config.query.as_deref().unwrap_or_default().split([',','，',';','；','\n']).any(|value|value.trim()==query) { bail!("navigation query must be configured"); }
        rt.block_on(browser::with_task_browser(|connection,list| Box::pin(async move {
            list.get(&offer_flow_lib::rpa::job51::filters::search_url(&runtime,&query)?)?;
            list.run_cdp("Page.bringToFront",None)?;
            let code=include_str!("../src/rpa/job51/ui.js").replace("export function ","function ");
            let mut found=false;
            for _ in 0..40 {
                let marked=list.run_js_await(&format!("(() => {{ {code} return markTitle51(document,{}); }})()",serde_json::to_string(&id)?))?;
                if marked.get("value").unwrap_or(&marked)==true { found=true; break; }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            if !found {
                println!("SEARCH_DIAGNOSTIC {}",list.run_js_await(&format!("(() => {{ {code} return {{title:document.title,path:location.pathname,ids:listedJobIds51(document)}}; }})()"))?);
                bail!("reviewed title is absent from current search page");
            }
            let before=connection.tabs()?.into_iter().map(|page|page.tab_id().to_owned()).collect::<std::collections::HashSet<_>>();
            list.click("[data-fj-51-title='1']")?;
            for _ in 0..30 {
                tokio::time::sleep(Duration::from_millis(300)).await;
                for page in connection.tabs()? {
                    if before.contains(page.tab_id()) { continue; }
                    let url=tauri::Url::parse(&page.url()?)?;
                    if url.host_str()!=Some("jobs.51job.com") || !url.path().ends_with(&format!("/{id}.html")) { continue; }
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    println!("NORMAL_JD_NAVIGATION {}",page.run_js_await("({title:document.title,path:location.pathname,referrer:document.referrer,hasOpener:!!window.opener,visibility:document.visibilityState,focus:document.hasFocus(),detailLoaded:!!document.querySelector('.job_msg')?.textContent?.trim()})")?);
                    return Ok(());
                }
            }
            bail!("reviewed job popup did not appear");
        })))?;
        return Ok(());
    }
    let env = match rt.block_on(run_flow::check_env(PlatformKind::Job51)) {
        Ok(env) => env,
        Err(error) => {
            let output_dir=output_dir.clone();
            rt.block_on(browser::with_browser(|connection| Box::pin(async move {
                let page=connection.tab();
                let info=page.run_js_await("({host:location.hostname, title:document.title, text:(document.body?.innerText||'').slice(0,800), images:Array.from(document.querySelectorAll('img,canvas')).map(e=>({tag:e.tagName,cls:e.className,parent:e.parentElement?.className,width:e.getBoundingClientRect().width,height:e.getBoundingClientRect().height}))})")?;
                println!("LOGIN_DIAGNOSTIC {}",info);
                let shot=page.run_cdp("Page.captureScreenshot",Some(json!({"format":"png"})))?;
                if let Some(data)=shot.get("data").and_then(serde_json::Value::as_str) {
                    use base64::Engine;
                    let path=output_dir.join("login-page.png");
                    std::fs::write(&path,base64::engine::general_purpose::STANDARD.decode(data)?)?;
                    println!("LOGIN_SCREENSHOT {}",path.display());
                }
                Ok(())
            })))?;
            return Err(error);
        }
    };
    if env.status != run_flow::EnvCheckStatus::Completed {
        if let Some(qr) = env.qr_code_base64 {
            use base64::Engine;
            let file = output_dir.join("login-qr.png");
            std::fs::write(&file, base64::engine::general_purpose::STANDARD.decode(qr)?)?;
            println!("LOGIN_REQUIRED {}", file.display());
        }
        bail!("51job登录尚未完成，请扫码后重新运行验收");
    }
    if mode == "login" {
        println!("LOGIN_CONFIRMED");
        return Ok(());
    }
    if mode == "inspect-pending" || mode == "reconcile-pending" {
        dao::init(&handle.path().app_data_dir()?)?;
        let reconcile=mode=="reconcile-pending";
        let mut reports=Vec::new();
        for id in std::env::args().nth(2).context("job IDs required")?.split(',') {
            let record=job_detail_dao::get_by_id(&format!("51job:{id}"))?.context("pending record missing")?;
            let url=record.source_url.context("original JD missing")?;
            let id=id.to_owned();
            let result=rt.block_on(browser::with_task_browser(|_,page| Box::pin(async move {
                page.get(&url)?;
                tokio::time::sleep(Duration::from_secs(3)).await;
                let code=include_str!("../src/rpa/job51/ui.js").replace("export function ","function ");
                let expression=format!("(() => {{ {code} return {{state:applyState51(document,{}),buttons:Array.from(document.querySelectorAll('.apply-btn-new')).map(e=>({{id:e.id,text:text51(e),classes:e.className}})),dialogs:Array.from(document.querySelectorAll('[role=dialog],.el-dialog__wrapper,.apply-component-resume-dialog,.attachment_resume_dialog')).filter(visible51).map(e=>({{classes:e.className,text:text51(e).slice(0,1600)}}))}}; }})()",serde_json::to_string(&id)?);
                let value=page.run_js_await(&expression)?;
                let value=value.get("value").cloned().unwrap_or(value);
                println!("PENDING_PAGE {} {}",id,value);
                if reconcile && value["state"]["kind"]=="already" {
                    let confirmed=offer_flow_lib::command::manual_review::job51_resolve_delivery(format!("51job:{id}"),true);
                    if !confirmed.success { bail!("已投递状态入库失败"); }
                    println!("SITE_CONFIRMED_PENDING {id}");
                }
                Ok(json!({"id":id,"site":value,"record":job_detail_dao::get_by_id(&format!("51job:{id}"))?}))
            })))?;
            reports.push(result);
        }
        std::fs::write(output_dir.join(format!("{mode}.json")),serde_json::to_vec_pretty(&reports)?)?;
        return Ok(());
    }
    if mode == "preview" {
        dao::init(&handle.path().app_data_dir()?)?;
        let candidates = rt.block_on(offer_flow_lib::rpa::job51::preview(&runtime, 3))?;
        let path=output_dir.join("preview.json");
        std::fs::write(&path,serde_json::to_vec_pretty(&candidates)?)?;
        println!("PREVIEW_REPORT {}",path.display());
        println!("{}",serde_json::to_string(&candidates)?);
        return Ok(());
    }
    if mode == "prepare-public" {
        dao::init(&handle.path().app_data_dir()?)?;
        if let Some(query)=std::env::args().nth(3) {
            let configured=runtime.job_filter_config.query.as_deref().unwrap_or_default();
            if !configured.split([',','，',';','；','\n']).any(|value|value.trim()==query) { bail!("公开读取关键词须来自当前求职方案"); }
            runtime.job_filter_config.query=Some(query);
        }
        let limit=std::env::args().nth(2).map(|value|value.parse::<usize>()).transpose()?.unwrap_or(3).clamp(1,20);
        let jobs=rt.block_on(offer_flow_lib::rpa::job51::prepare_public_jobs(&runtime,limit))?;
        let report=output_dir.join("public-jobs.json");
        std::fs::write(&report,serde_json::to_vec_pretty(&jobs)?)?;
        println!("PUBLIC_JOBS_REPORT {}",report.display());
        println!("{}",serde_json::to_string(&jobs.iter().map(|job|json!({"title":job.title,"company":job.company_name,"url":job.detail_url})).collect::<Vec<_>>())?);
        return Ok(());
    }
    let (flow, limit) = match mode.as_str() {
        "single" => (FlowMode::JobHunting, 1usize),
        "periodic" => (FlowMode::PeriodicJobHunting, 2usize),
        _ => bail!("use inspect, login, single or periodic"),
    };
    // Optional acceptance priority retains every configured keyword and all filters.
    if let Some(priority)=std::env::args().nth(3) {
        let mut queries=runtime.job_filter_config.query.as_deref().unwrap_or_default()
            .split([',','，',';','；','\n']).map(str::trim).filter(|query|!query.is_empty()).collect::<Vec<_>>();
        let position=queries.iter().position(|query|*query==priority.as_str()).context("priority must be a configured keyword")?;
        queries.remove(position);
        queries.insert(0,priority.as_str());
        runtime.job_filter_config.query=Some(queries.join(", "));
    }
    let data_dir = handle.path().app_data_dir()?;
    dao::init(&data_dir)?;
    let snapshot =
        JobProfileSnapshot::from_resolved(&runtime).context("profile snapshot missing")?;
    let profile = JobTaskProfile {
        profile_id: Some(snapshot.profile_id.clone()),
        profile_name: Some(snapshot.profile_name.clone()),
        profile_snapshot_id: Some(snapshot.snapshot_id.clone()),
    };
    profile_snapshot_dao::upsert(snapshot)?;
    // Keep the user's strategy, resume and configured pacing unchanged.
    runtime.analysis_config.trigger = config::AnalysisTrigger::Off;
    let plan = (flow == FlowMode::PeriodicJobHunting).then(|| PeriodicPlan {
        max_greets_per_round: 1,
        max_round_minutes: 4,
        run_until: Some(chrono::Local::now() + chrono::Duration::minutes(12)),
        ..PeriodicPlan::every(1)
    });
    let task = JOB_TASK_MANAGER
        .submit(PlatformKind::Job51, flow, plan, runtime, Some(profile))
        .map_err(anyhow::Error::msg)?;
    println!("TASK_STARTED {}", task.task_id);
    let started = Instant::now();
    let mut stopped = false;
    let mut previous_count = 0;
    loop {
        let jobs = job_detail_dao::list()?
            .into_iter()
            .filter(|job| {
                job.source_task_id.as_deref() == Some(&task.task_id) && job.is_send_resume
            })
            .collect::<Vec<_>>();
        if jobs.len() != previous_count {
            println!("CONFIRMED_DELIVERIES {}", jobs.len());
            previous_count = jobs.len();
        }
        if !stopped && (jobs.len() >= limit || started.elapsed() > Duration::from_secs(12 * 60)) {
            JOB_TASK_MANAGER
                .stop(&task.task_id)
                .map_err(anyhow::Error::msg)?;
            stopped = true;
        }
        let info = JOB_TASK_MANAGER
            .overview()
            .tasks
            .into_iter()
            .find(|item| item.task_id == task.task_id)
            .context("task missing")?;
        if matches!(
            info.status,
            JobTaskState::Succeeded | JobTaskState::Failed | JobTaskState::Cancelled
        ) {
            let summary = json!({"task":info,"confirmed_deliveries":jobs.len(),"jobs":jobs.iter().map(|job|json!({"id":job.id,"title":job.title,"company":job.company_name,"source_url":job.source_url,"sent_at":job.resume_sent_at})).collect::<Vec<_>>()});
            let report = output_dir.join(format!("{mode}.json"));
            std::fs::write(&report, serde_json::to_vec_pretty(&summary)?)?;
            println!("REPORT {}", report.display());
            if jobs.len() != limit || jobs.iter().any(|job| job.source_url.is_none()) {
                bail!("live acceptance incomplete: confirmed {} of {limit} applications; task status {:?}",jobs.len(),info.status);
            }
            println!("ACCEPTANCE_PASSED {mode}");
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}
