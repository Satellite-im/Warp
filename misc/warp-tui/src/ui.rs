use crate::WarpApp;
use tui::backend::Backend;
use tui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use tui::style::{Color, Modifier, Style};
use tui::text::{Span, Spans};
use tui::widgets::{Block, BorderType, Borders, List, ListItem, Paragraph, Tabs, Wrap};
use tui::Frame;
use tui_logger::{TuiLoggerLevelOutput, TuiLoggerWidget};

impl<'a> WarpApp<'a> {
    pub fn draw_ui<B: Backend>(&mut self, frame: &mut Frame<B>) {
        // let size = frame.size();
        //
        // let block = Block::default()
        //     .borders(Borders::ALL)
        //     .title(self.title)
        //     .title_alignment(Alignment::Center)
        //     .border_type(BorderType::Rounded)
        //     .border_style(Style::default().fg(Color::White).bg(Color::White));
        // frame.render_widget(block, size);

        let layout = Layout::default()
            .constraints(vec![Constraint::Length(3), Constraint::Min(0)])
            .split(frame.size());

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
            1 => self.draw_config(frame, layout[1]),
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
            self.draw_tools(frame, layout[1]);
        }
        self.draw_extensions(frame, layout[1]);
    }

    pub fn draw_loop<B: Backend>(&self, frame: &mut Frame<B>, area: Rect) {
        //TODO
    }

    pub fn draw_modules_and_hooks<B: Backend>(&self, frame: &mut Frame<B>, area: Rect) {
        let layout = Layout::default()
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
            .direction(Direction::Horizontal)
            .split(area);

        let modules: Vec<ListItem> = self
            .modules
            .modules
            .iter()
            .map(|m| {
                let (module, active) = m;
                let style = match active {
                    true => Style::default().fg(Color::Green),
                    false => Style::default().fg(Color::Red),
                };
                ListItem::new(vec![Spans::from(Span::styled(
                    format!("{:<9}", module.to_string()),
                    style,
                ))])
            })
            .collect();

        let modules_list =
            List::new(modules).block(Block::default().borders(Borders::ALL).title("Modules"));

        frame.render_widget(modules_list, layout[0]);

        let hooks: Vec<ListItem> = self
            .hook_system
            .hooks()
            .iter()
            .map(|i| ListItem::new(vec![Spans::from(Span::from(i.to_string()))]))
            .collect();

        let hooks_list = List::new(hooks).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Registered Hooks"),
        );

        frame.render_widget(hooks_list, layout[1]);
    }

    pub fn draw_tools<B: Backend>(&mut self, frame: &mut Frame<B>, area: Rect) {
        let layout = Layout::default()
            .constraints([Constraint::Percentage(100)].as_ref())
            .direction(Direction::Horizontal)
            .split(area);

        let hooks: Vec<ListItem> = self
            .tools
            .list
            .iter()
            .map(|i| ListItem::new(vec![Spans::from(Span::from(i.to_string()))]))
            .collect();

        let list = List::new(hooks)
            .block(Block::default().borders(Borders::ALL).title("Tools"))
            .highlight_style(Style::default().add_modifier(Modifier::BOLD))
            .highlight_symbol(">> ");

        frame.render_stateful_widget(list, layout[0], &mut self.tools.state);
    }
    pub fn draw_modules<B: Backend>(&self, frame: &mut Frame<B>, area: Rect) {
        let layout = Layout::default()
            .constraints([Constraint::Percentage(100)].as_ref())
            .direction(Direction::Horizontal)
            .split(area);

        let hooks: Vec<ListItem> = vec![""]
            .iter()
            .map(|i| ListItem::new(vec![Spans::from(Span::from(i.to_string()))]))
            .collect();

        let list = List::new(hooks)
            .block(Block::default().borders(Borders::ALL).title("Extensions"))
            .highlight_style(Style::default().add_modifier(Modifier::BOLD))
            .highlight_symbol(">> ");

        frame.render_widget(list, layout[0]);
    }

    pub fn draw_extensions<B: Backend>(&self, frame: &mut Frame<B>, area: Rect) {
        let layout = Layout::default()
            .constraints([Constraint::Percentage(100)].as_ref())
            .direction(Direction::Horizontal)
            .split(area);

        let hooks: Vec<ListItem> = vec![""]
            .iter()
            .map(|i| ListItem::new(vec![Spans::from(Span::from(i.to_string()))]))
            .collect();

        let list = List::new(hooks)
            .block(Block::default().borders(Borders::ALL).title("Extensions"))
            .highlight_style(Style::default().add_modifier(Modifier::BOLD))
            .highlight_symbol(">> ");

        frame.render_widget(list, layout[0]);
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

    pub fn main_ui(&self) -> anyhow::Result<()> {
        Ok(())
    }
}
