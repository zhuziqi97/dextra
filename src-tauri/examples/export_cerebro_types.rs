use codeg_lib::cerebro::{identity::CerebroTargetBinding, session_binding::{CerebroSelection, CerebroLaunchBinding}};
use ts_rs::TS;

fn main() -> Result<(), ts_rs::ExportError> {
    // 从生产 DTO 派生前端类型，不在测试中维护字段镜像。
    let output = "../src/lib/generated/cerebro";
    CerebroSelection::export_all_to(output)?;
    CerebroTargetBinding::export_all_to(output)?;
    CerebroLaunchBinding::export_all_to(output)?;
    Ok(())
}
