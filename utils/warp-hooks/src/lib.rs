use warp_data::DataType;

pub mod hooks;

pub trait HookType: std::fmt::Display {
    fn module_type(&self) -> DataType {
        DataType::Unknown
    }

    fn base_type(&self) -> String;

    fn hook_type(&self) -> String {
        format!("{}", self).to_lowercase()
    }
}
