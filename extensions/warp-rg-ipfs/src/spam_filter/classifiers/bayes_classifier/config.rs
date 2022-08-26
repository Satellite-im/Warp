use super::FILE_NAME;

pub struct Config {
    pub file_name: String,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            file_name: FILE_NAME.to_string()
        }
    }
}
