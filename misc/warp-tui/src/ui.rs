use crate::WarpApp;
use tui::backend::Backend;
use tui::layout::{Alignment, Constraint, Direction, Layout};
use tui::style::{Color, Style};
use tui::widgets::{Block, BorderType, Borders, Paragraph};
use tui::Frame;

impl WarpApp {
    pub fn draw_ui<B: Backend>(&self, rect: &mut Frame<B>) {
        let size = rect.size();

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1)].as_ref())
            .split(size);

        let title = Paragraph::new(self.title.as_ref())
            .style(Style::default().fg(Color::Green))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .style(Style::default().fg(Color::Green))
                    .border_type(BorderType::Plain),
            );

        rect.render_widget(title, layout[0]);
    }

    pub fn main_ui(&self) -> anyhow::Result<()> {
        Ok(())
    }
}
