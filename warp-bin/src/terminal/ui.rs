use super::WarpApp;
use tui::backend::Backend;
use tui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use tui::style::{Color, Modifier, Style};
use tui::text::{Span, Spans};
use tui::widgets::{Block, BorderType, Borders, List, ListItem, Paragraph, Row, Table, Tabs, Wrap};
use tui::Frame;
use tui_logger::{TuiLoggerLevelOutput, TuiLoggerWidget};
use warp::constellation::item::Item;
use warp::data::DataType;

impl<'a> WarpApp<'a> {
    pub fn draw_ui<B: Backend>(&mut self, frame: &mut Frame<B>) {
        let layout = match std::env::args().collect::<Vec<String>>().get(1) {
            Some(arg) if arg.eq("--no-border") => Layout::default()
                .constraints(vec![Constraint::Length(3), Constraint::Min(0)])
                .split(frame.size()),
            _ => {
                let size = frame.size();

                let block = Block::default()
                    .borders(Borders::ALL)
                    .title(self.title)
                    .title_alignment(Alignment::Center)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::White).bg(Color::Black));
                frame.render_widget(block, size);

                Layout::default()
                    .margin(4)
                    .constraints(vec![Constraint::Length(3), Constraint::Min(0)])
                    .split(frame.size())
            }
        };

        let titles = self
            .tabs
            .titles
            .iter()
            .map(|t| Spans::from(Span::styled(*t, Style::default().fg(Color::Green))))
            .collect();

        let tabs = Tabs::new(titles)
            .block(Block::default().borders(Borders::ALL).title("Menu"))
            .highlight_style(Style::default().fg(Color::Yellow))
            .select(self.tabs.index);

        frame.render_widget(tabs, layout[0]);

        match self.tabs.index {
            0 => self.draw_main(frame, layout[1]),
            1 => self.draw_extensions(frame, layout[1]),
            2 => self.draw_config(frame, layout[1]),
            _ => {}
        };
    }

    pub fn draw_main<B: Backend>(&mut self, frame: &mut Frame<B>, area: Rect) {
        let layout = Layout::default()
            .constraints(vec![Constraint::Percentage(80), Constraint::Percentage(20)])
            .split(area);
        self.draw_middle(frame, layout[0]);
        self.draw_logs(frame, layout[1])
    }

    pub fn draw_config<B: Backend>(&mut self, frame: &mut Frame<B>, area: Rect) {
        let layout = Layout::default()
            .constraints(vec![Constraint::Percentage(80), Constraint::Percentage(20)])
            .split(area);
        {
            let layout = Layout::default()
                .constraints([Constraint::Percentage(20), Constraint::Percentage(80)].as_ref())
                .direction(Direction::Horizontal)
                .split(layout[0]);

            let hooks: Vec<ListItem> = self
                .config
                .menu()
                .iter()
                .map(|i| ListItem::new(vec![Spans::from(Span::from(i.to_string()))]))
                .collect();

            let list = List::new(hooks)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Configuration"),
                )
                .highlight_style(Style::default().add_modifier(Modifier::BOLD))
                .highlight_symbol(">> ");

            frame.render_stateful_widget(list, layout[0], &mut self.config.state);
        }
        self.draw_logs(frame, layout[1])
    }

    pub fn draw_middle<B: Backend>(&mut self, frame: &mut Frame<B>, area: Rect) {
        let layout = Layout::default()
            .constraints(vec![Constraint::Percentage(50), Constraint::Percentage(50)])
            .direction(Direction::Horizontal)
            .split(area);

        {
            let layout = Layout::default()
                .constraints(vec![Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(layout[0]);
            self.draw_modules_and_hooks(frame, layout[0]);
            self.draw_tools_and_cache(frame, layout[1]);
        }
        self.draw_filesystem(frame, layout[1]);
    }

    pub fn draw_loop<B: Backend>(&self, _: &mut Frame<B>, _: Rect) {
        //TODO
    }

    pub fn draw_modules_and_hooks<B: Backend>(&self, frame: &mut Frame<B>, area: Rect) {
        let layout = Layout::default()
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
            .direction(Direction::Horizontal)
            .split(area);

        let rows = self.modules.modules.iter().map(|m| {
            let (module, active) = m;
            let style = match active {
                true => Style::default().fg(Color::Green),
                false => Style::default().fg(Color::Red),
            };

            Row::new(vec![
                module.to_string().to_lowercase(),
                format!("{}", active),
            ])
            .style(style)
        });

        let table = Table::new(rows)
            .header(
                Row::new(vec!["Module Name", "Active"])
                    .style(Style::default().fg(Color::White))
                    .bottom_margin(1),
            )
            .block(Block::default().title("Modules").borders(Borders::ALL))
            .widths(&[Constraint::Length(15), Constraint::Length(15)]);

        frame.render_widget(table, layout[0]);

        let hooks: Vec<ListItem> = self
            .hooks_trigger
            .clone()
            .lock()
            .iter()
            .map(|i| ListItem::new(vec![Spans::from(Span::from(i.to_string()))]))
            .collect();

        let hooks_list =
            List::new(hooks).block(Block::default().borders(Borders::ALL).title("Hooks"));

        frame.render_widget(hooks_list, layout[1]);
    }

    pub fn draw_tools_and_cache<B: Backend>(&mut self, frame: &mut Frame<B>, area: Rect) {
        let layout = Layout::default()
            .constraints([Constraint::Percentage(20), Constraint::Percentage(80)].as_ref())
            .direction(Direction::Horizontal)
            .split(area);

        let list: Vec<ListItem> = self
            .tools
            .list
            .iter()
            .map(|i| ListItem::new(vec![Spans::from(Span::from(i.to_string()))]))
            .collect();

        let list = List::new(list)
            .block(Block::default().borders(Borders::ALL).title("Tools"))
            .highlight_style(Style::default().add_modifier(Modifier::BOLD))
            .highlight_symbol(">> ");

        frame.render_stateful_widget(list, layout[0], &mut self.tools.state);
        self.draw_cache(frame, layout[1])
    }

    pub fn draw_modules<B: Backend>(&self, _: &mut Frame<B>, _: Rect) {}

    pub fn draw_filesystem<B: Backend>(&self, frame: &mut Frame<B>, area: Rect) {
        let layout = Layout::default()
            .constraints([Constraint::Percentage(90), Constraint::Percentage(10)].as_ref())
            .direction(Direction::Vertical)
            .split(area);

        let rows = self
            .filesystem
            .as_ref()
            .unwrap()
            .root_directory()
            .get_items()
            .iter()
            // .filter(|item| item.is_file())
            .map(|item| {
                Row::new(vec![
                    item.name(),
                    format!("{}", item.size()),
                    format!("{}", item.creation()),
                    if item.is_file() {
                        if let Ok(file) = item.get_file() {
                            file.hash().sha256().unwrap_or_default()
                        } else {
                            String::new()
                        }
                    } else {
                        String::new()
                    },
                ])
                .style(Style::default().fg(Color::Green))
            });

        let table = Table::new(rows)
            .header(
                Row::new(vec!["Name", "Size", "Date", "Hash"])
                    .style(Style::default().fg(Color::White))
                    .bottom_margin(1),
            )
            .block(Block::default().title("Filesystem").borders(Borders::ALL))
            .widths(&[
                Constraint::Length(15),
                Constraint::Length(10),
                Constraint::Length(35),
                Constraint::Length(65),
            ]);

        frame.render_widget(table, layout[0]);
    }

    pub fn draw_cache<B: Backend>(&mut self, frame: &mut Frame<B>, area: Rect) {
        let layout = Layout::default()
            .constraints([Constraint::Percentage(100)].as_ref())
            .direction(Direction::Horizontal)
            .split(area);

        let cache = match self.cache.as_ref() {
            Some(cache) => cache.lock(),
            None => return,
        };

        let rows = self
            .modules
            .modules
            .iter()
            .filter(|(_, active)| *active)
            .map(|(module, _)| module)
            .map(|module| {
                let data = cache
                    .get_data(DataType::from(module.clone()), None)
                    .unwrap_or_default();
                Row::new(vec![
                    module.to_string().to_lowercase(),
                    format!("{}", data.len()),
                ])
                .style(Style::default().fg(Color::Green))
            });

        let table = Table::new(rows)
            .header(
                Row::new(vec!["Module", "Amount Cached"])
                    .style(Style::default().fg(Color::White))
                    .bottom_margin(1),
            )
            .block(Block::default().title("Cache").borders(Borders::ALL))
            .widths(&[Constraint::Length(15), Constraint::Length(15)]);

        frame.render_widget(table, layout[0]);
    }

    pub fn draw_logs<B: Backend>(&self, frame: &mut Frame<B>, area: Rect) {
        let tui_logger: TuiLoggerWidget = TuiLoggerWidget::default()
            .block(
                Block::default()
                    .title("Logs")
                    .border_style(Style::default().fg(Color::White).bg(Color::Black))
                    .borders(Borders::ALL),
            )
            .output_separator('|')
            .output_timestamp(Some("%F %H:%M:%S%.3f".to_string()))
            .output_level(Some(TuiLoggerLevelOutput::Long))
            .output_target(false)
            .output_file(false)
            .output_line(false)
            .style(Style::default().fg(Color::White).bg(Color::Black));
        frame.render_widget(tui_logger, area);
    }

    pub fn draw_extensions<B: Backend>(&mut self, frame: &mut Frame<B>, area: Rect) {
        let layout = Layout::default()
            .constraints(vec![Constraint::Percentage(80), Constraint::Percentage(20)])
            .split(area);
        {
            let layout = Layout::default()
                .constraints([Constraint::Percentage(20), Constraint::Percentage(80)].as_ref())
                .direction(Direction::Horizontal)
                .split(layout[0]);

            let ext: Vec<ListItem> = self
                .extensions
                .list
                .iter()
                .map(|i| ListItem::new(vec![Spans::from(Span::from(i.name()))]))
                .collect();

            let list = List::new(ext)
                .block(Block::default().borders(Borders::ALL).title("Extensions"))
                .highlight_style(Style::default().add_modifier(Modifier::BOLD))
                .highlight_symbol(">> ");

            frame.render_stateful_widget(list, layout[0], &mut self.extensions.state);
            let text = match self
                .extensions
                .state
                .selected()
                .and_then(|index| self.extensions.list.get(index))
                .map(|info| (info.name(), info.description()))
            {
                Some((name, desc)) => {
                    vec![
                        Spans::from(""),
                        Spans::from(format!("Extension name: {}", name)),
                        Spans::from(format!("Description: {}", desc)),
                    ]
                }
                None => Vec::new(),
            };

            let block = Block::default().borders(Borders::ALL).title(Span::styled(
                "Extension Information",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ));
            let paragraph = Paragraph::new(text).block(block).wrap(Wrap { trim: true });

            frame.render_widget(paragraph, layout[1]);
        }
        self.draw_logs(frame, layout[1])
    }

    pub fn main_ui<B: Backend>(&mut self, _: &mut Frame<B>) {}
}
