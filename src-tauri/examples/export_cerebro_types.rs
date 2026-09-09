use ts_rs::TS;
use codeg_lib::cerebro::configuration::*;

fn main() -> Result<(), ts_rs::ExportError> {
    // 从生产 DTO 派生前端类型，不在测试中维护字段镜像。
    let output = "../src/lib/generated/cerebro";
    ClientConfiguration::export_all_to(output)?;
    ConfigurationInput::export_all_to(output)?;
    FolderConfigurationState::export_all_to(output)?;
    ProjectPage::export_all_to(output)?;
    ModuleOptions::export_all_to(output)?;
    Ok(())
}
