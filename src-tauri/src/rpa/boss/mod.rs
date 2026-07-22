pub mod handler;
pub mod model;

// 页面
pub const BOSS_LOGIN_PAGE_URL: &str = "https://www.zhipin.com/web/user/?ka=header-login";

// token认证接口
pub const BOSS_ACCOUNT_VERIFY_API: &str = "https://www.zhipin.com/wapi/zpboss/h5/user/info";

// Boss 首页
pub const BOSS_SITE_URL: &str = "https://www.zhipin.com";

// 沟通页
pub const BOSS_CHAT_URL: &str = "https://www.zhipin.com/web/geek/chat";

pub fn rpa_flaw() -> Result<(), anyhow::Error> {
    Ok(())
}
