use std::borrow::Cow::{self, Borrowed, Owned};

use rustyline::highlight::Highlighter;
use rustyline_derive::{Completer, Helper, Hinter, Validator};
use unicode_width::UnicodeWidthStr;
use warp_common::anyhow;
#[derive(Default, Completer, Helper, Hinter, Validator)]
pub struct UnsecuredMarker {
    hide: bool,
}

impl UnsecuredMarker {
    pub fn flip(&mut self) {
        self.hide = !self.hide;
    }
}

impl Highlighter for UnsecuredMarker {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        if self.hide {
            Owned("*".repeat(line.width()))
        } else {
            Borrowed(line)
        }
    }

    fn highlight_char(&self, _line: &str, _pos: usize) -> bool {
        self.hide
    }
}

pub fn command_line() -> anyhow::Result<()> {
    Ok(())
}
