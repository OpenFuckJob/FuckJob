pub mod handler;

pub const LIEPIN_SITE_URL: &str = "https://www.liepin.com";
pub const LIEPIN_LOGIN_PAGE_URL: &str = "https://www.liepin.com/simple-login/####";
pub const LIEPIN_CHAT_URL: &str = "https://www.liepin.com/message/";
/// 候选人端主页。用户信息接口只接受来自这个域的请求，登录校验必须站在这里发起
pub const LIEPIN_USER_HOME_URL: &str = "https://c.liepin.com";
pub const LIEPIN_USER_PROPERTY_API: &str =
    "https://api-c.liepin.com/api/com.liepin.usercx.pc.user.base-property";
