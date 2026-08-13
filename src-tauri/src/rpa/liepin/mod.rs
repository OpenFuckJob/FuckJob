pub mod handler;

pub const LIEPIN_SITE_URL: &str = "https://www.liepin.com";
pub const LIEPIN_LOGIN_PAGE_URL: &str = "https://www.liepin.com/simple-login/####";
/// 候选人端主页。用户信息接口只接受来自这个域的请求，登录校验必须站在这里发起。
///
/// 沟通也在这个页面：猎聘 C 端没有独立聊天页
/// （`www.liepin.com/message/` 实测 404），会话是首页右侧的抽屉面板。
pub const LIEPIN_USER_HOME_URL: &str = "https://c.liepin.com";
pub const LIEPIN_USER_PROPERTY_API: &str =
    "https://api-c.liepin.com/api/com.liepin.usercx.pc.user.base-property";
