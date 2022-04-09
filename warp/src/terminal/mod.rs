pub mod base;
pub mod cache;
pub mod filesystem;

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[allow(unused_imports)]
use crossterm::event::{self, Event, KeyCode};
#[allow(unused_imports)]
use tui::{
    backend::Backend,
    layout::{Alignment, Constraint, Layout},
    style::{Color, Style},
    text::{Span, Spans},
    widgets::{Block, BorderType, Borders, ListState, Tabs},
    Frame, Terminal,
};
use warp_common::{anyhow, Module};

use crate::manager::ModuleManager;

#[derive(Default, Clone)]
pub struct TerminalApplication {
    pub title: Option<String>,
    pub modules: Vec<Module>,
    pub module_manager: Arc<Mutex<ModuleManager>>,
    pub config: List<(Module, bool)>,
    pub tools: List<String>,
}

impl TerminalApplication {
    pub fn new() -> anyhow::Result<Self> {
        let mut terminal = TerminalApplication::default();
        terminal.title = Some("Warp by Satellite".to_string());
        Ok(terminal)
    }
    pub async fn start_ui(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    pub async fn start_loop<B: Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
    ) -> anyhow::Result<()> {
        let mut last_tick = Instant::now();
        let tick_rate = Duration::from_secs(250);
        loop {
            terminal.draw(|f| self.draw_base(f))?;

            let timeout = tick_rate
                .checked_sub(last_tick.elapsed())
                .unwrap_or_else(|| Duration::from_secs(0));
            if crossterm::event::poll(timeout)? {
                if let Event::Key(key) = event::read()? {
                    match key.code {
                        // KeyCode::Char(c) => app.key_press(c),
                        // KeyCode::Left => app.left(),
                        // KeyCode::Up => app.up(),
                        // KeyCode::Right => app.right(),
                        // KeyCode::Down => app.down(),
                        // KeyCode::Enter => app.select().await,
                        // KeyCode::Esc => return Ok(()),
                        _ => {}
                    }
                }
            }
            if last_tick.elapsed() >= tick_rate {
                // self.config.list = self.modules.modules.clone();
                //perform any updates during this tick
                last_tick = Instant::now();
            }
            // if app.exit {
            //     return Ok(());
            // }
        }
    }
}

impl TerminalApplication {
    pub fn draw_base<B: Backend>(&mut self, _: &mut Frame<B>) {
        // let layout = match std::env::args().collect::<Vec<String>>().get(1) {
        //     Some(arg) if arg.eq("--no-border") => Layout::default()
        //         .constraints(vec![Constraint::Length(3), Constraint::Min(0)])
        //         .split(frame.size()),
        //     _ => {
        //         let size = frame.size();

        //         let block = Block::default()
        //             .borders(Borders::ALL)
        //             .title(self.title.unwrap_or_default())
        //             .title_alignment(Alignment::Center)
        //             .border_type(BorderType::Rounded)
        //             .border_style(Style::default().fg(Color::White).bg(Color::Black));
        //         frame.render_widget(block, size);

        //         Layout::default()
        //             .margin(4)
        //             .constraints(vec![Constraint::Length(3), Constraint::Min(0)])
        //             .split(frame.size())
        //     }
        // };

        // let titles = self
        //     .tabs
        //     .titles
        //     .iter()
        //     .map(|t| Spans::from(Span::styled(*t, Style::default().fg(Color::Green))))
        //     .collect();

        // let tabs = Tabs::new(titles)
        //     .block(Block::default().borders(Borders::ALL).title("Menu"))
        //     .highlight_style(Style::default().fg(Color::Yellow))
        //     .select(self.tabs.index);

        // frame.render_widget(tabs, layout[0]);

        // match self.tabs.index {
        //     // 0 => self.draw_main(frame, layout[1]),
        //     // 1 => self.draw_extensions(frame, layout[1]),
        //     // 2 => self.draw_config(frame, layout[1]),
        //     _ => {}
        // };
    }
}

#[derive(Default, Clone)]
pub struct List<T> {
    pub state: ListState,
    pub list: Vec<T>,
}

impl<T: Default> List<T> {
    pub fn new() -> Self {
        Self::default()
    }
}

impl<T> List<T> {
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
