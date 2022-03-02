pub mod ui;

use warp_hooks::hooks::Hooks;
use warp_module::Module;

#[derive(Default)]
pub struct WarpApp {
    pub title: String,
    pub hook_system: Hooks,
}

impl WarpApp {
    pub fn new<S: AsRef<str>>(title: S) -> anyhow::Result<Self> {
        let mut app = WarpApp::default();
        app.title = title.as_ref().to_string();

        app.hook_system = {
            let mut hook_system = Hooks::default();

            // Register different qualified hooks TODO: Implement a function to register multiple hooks from a vector
            // filesystem hooks
            hook_system.create("NEW_FILE", Module::FileSystem)?;
            hook_system.create("NEW_DIRECTORY", Module::FileSystem)?;
            hook_system.create("DELETE_FILE", Module::FileSystem)?;
            hook_system.create("DELETE_DIRECTORY", Module::FileSystem)?;
            hook_system.create("MOVE_FILE", Module::FileSystem)?;
            hook_system.create("MOVE_DIRECTORY", Module::FileSystem)?;
            hook_system.create("RENAME_FILE", Module::FileSystem)?;
            hook_system.create("RENAME_DIRECTORY", Module::FileSystem)?;

            hook_system
        };

        Ok(app)
    }

    pub fn up(&mut self) {}
    pub fn down(&mut self) {}
    pub fn left(&mut self) {}
    pub fn right(&mut self) {}
    pub fn key_press(&mut self, _: char) {
        // match key {
        //     `q` =>
        // }
    }
}

#[tokio::main]
async fn main() -> warp_common::Result<()> {
    //

    Ok(())
}
