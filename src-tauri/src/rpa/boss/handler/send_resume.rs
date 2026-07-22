// 发送简历

use std::time::Duration;

use rust_drission::Page;

pub fn send_resume(page: &Page) -> Result<bool, anyhow::Error> {
    let send_btn_selector = ".toolbar-btn";
    let send_btn_eles = page.eles(send_btn_selector)?;
    for send_btn in send_btn_eles {
        if send_btn.text_content()?.trim() == "发简历" {
            send_btn.click()?;
            page.wait(".panel-resume", Duration::from_secs(5))?;
            let btn_span_eles = page.elements(".panel-resume .btns span")?;
            for btn_span_ele in btn_span_eles {
                if btn_span_ele.text_content()?.trim() == "确定" {
                    btn_span_ele.click()?;
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}
