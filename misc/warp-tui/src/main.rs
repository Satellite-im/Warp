pub mod ui;

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use log::{error, info, warn, LevelFilter};
use std::io;
use std::time::{Duration, Instant};
use tui::backend::{Backend, CrosstermBackend};
use tui::widgets::ListState;
use tui::Terminal;
use tui_logger::{init_logger, set_default_level};
use warp_hooks::hooks::Hooks;
use warp_module::Module;
use warp_pocket_dimension::PocketDimension;

#[derive(Default)]
pub struct WarpApp<'a> {
    pub title: &'a str,
    pub hook_system: Hooks,
    //TODO: Implement cacher
    // pub cache: Option<Box<dyn PocketDimension>>,
    pub modules: Modules,
    pub tools: Tools,
    pub tabs: Tabs<'a>,
    pub exit: bool,
}

#[derive(Default)]
pub struct Tools {
    pub state: ListState,
    pub list: Vec<String>,
}

impl Tools {
    pub fn new(list: Vec<String>) -> Self {
        Tools {
            list,
            ..Default::default()
        }
    }

    pub fn next(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.list.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    pub fn previous(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.list.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }
}

#[derive(Default)]
pub struct Tabs<'a> {
    pub titles: Vec<&'a str>,
    pub index: usize,
}

impl<'a> Tabs<'a> {
    pub fn new(titles: Vec<&'a str>) -> Tabs {
        Tabs { titles, index: 0 }
    }

    pub fn next(&mut self) {
        self.index = (self.index + 1) % self.titles.len();
    }

    pub fn previous(&mut self) {
        if self.index > 0 {
            self.index -= 1;
        } else {
            self.index = self.titles.len() - 1;
        }
    }
}

#[derive(Default)]
pub struct Modules {
    pub modules: Vec<(Module, bool)>,
}

impl Modules {
    pub fn new() -> Self {
        let mut modules = Modules::default();
        //Default values
        modules.modules = vec![
            (Module::FileSystem, true),
            (Module::Cache, true),
            (Module::Accounts, false),
            (Module::Messaging, false),
        ];
        modules
    }
}

impl<'a> WarpApp<'a> {
    pub fn new(title: &'a str) -> anyhow::Result<Self> {
        let mut app = WarpApp::default();
        app.title = title.as_ref();

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

            // pocketdimension hooks
            //TODO

            hook_system
        };

        app.tabs = Tabs::new(vec!["Main", "Config"]);
        app.tools = Tools::new(
            vec!["Load Mock Data", "Clear Cache", "Start", "Stop", "Restart"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        );

        app.modules = Modules::new();
        Ok(app)
    }

    pub fn up(&mut self) {
        self.tools.previous()
    }
    pub fn down(&mut self) {
        self.tools.next()
    }
    pub fn left(&mut self) {
        self.tabs.previous()
    }
    pub fn right(&mut self) {
        self.tabs.next()
    }
    pub fn select(&mut self) {
        if self.tabs.index != 0 {
            return;
        }
        match self.tools.state.selected() {
            Some(selected) => {
                if let Some(item) = self.tools.list.get(selected).map(|item| item.as_str()) {
                    match item {
                        "Load Mock Data" => {
                            info!(target:"Warp", "Loading data...")
                        }
                        "Clear Cache" => {
                            info!(target:"Warp", "Clearing cache...")
                        }
                        other => {
                            error!(target:"Error", "'{}' is currently disabled or not a valid option", other)
                        }
                    }
                }
            }
            None => error!(target:"Error", "State is invalid"),
        }
    }
    pub fn key_press(&mut self, key: char) {
        match key {
            'q' => self.exit = true,
            k => {
                warn!(target:"Warn", "Key '{}' is invalid", k)
            }
        }
    }
}

async fn run_ui() -> anyhow::Result<()> {
    info!(target:"Warp", "Initializing interface");
    enable_raw_mode()?;
    let mut stdout = io::stdout();

    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    let backend = CrosstermBackend::new(stdout);

    let mut terminal = Terminal::new(backend)?;

    let mut warp_main = WarpApp::new("Warp by Satellite")?;

    let run = run_loop(&mut terminal, &mut warp_main).await;
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = run {
        let error = format!("{:?}", err);
        error!(target:"Error", "{}", error)
    }
    Ok(())
}

async fn run_loop<'a, B: Backend>(
    terminal: &mut Terminal<B>,
    app: &mut WarpApp<'a>,
) -> anyhow::Result<()> {
    let mut last_tick = Instant::now();
    let tick_rate = Duration::from_secs(250);
    loop {
        // Workaround. TODO: Investigate why terminal is not clearing
        terminal.clear()?;
        terminal.draw(|f| app.draw_ui(f))?;

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));
        if crossterm::event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char(c) => app.key_press(c),
                    KeyCode::Left => app.left(),
                    KeyCode::Up => app.up(),
                    KeyCode::Right => app.right(),
                    KeyCode::Down => app.down(),
                    KeyCode::Enter => app.select(),
                    KeyCode::Esc => return Ok(()),
                    _ => {}
                }
            }
        }
        if last_tick.elapsed() >= tick_rate {
            //perform any updates during this tick
            last_tick = Instant::now();
        }
        if app.exit {
            return Ok(());
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_logger(LevelFilter::Info).unwrap();
    set_default_level(LevelFilter::Trace);
    info!(target:"Warp", "Starting Warp Terminal Interface");
    run_ui().await
}
