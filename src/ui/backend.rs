//! Backend wrapper which keeps the terminal in sync with ratatui's buffer when the terminal
//! disagrees about the width of some characters

use std::io;
use std::iter;

use ratatui::backend::{Backend, ClearType, WindowSize};
use ratatui::buffer::{Cell, CellWidth};
use ratatui::layout::{Position, Size};

/// Wraps a backend to make rendering robust against character width disagreements
///
/// ratatui computes the width of each grapheme with `unicode-width`, while the terminal uses its
/// own tables. They disagree for some emoji sequences, skin tone modifiers, variation selectors and
/// some scripts; e.g. tmux 3.7 renders `✌🏽` 4 columns wide, but `unicode-width` says 2. ratatui
/// only redraws changed cells, and the wrapped backend moves the cursor only between
/// non-contiguous cells. So after a glyph with a disagreeing width:
///
/// * the following text is printed shifted, if the terminal's glyph is wider, and
/// * cells, which ratatui considers covered by a wide glyph, are never written, if the terminal's
///   glyph is narrower; old characters stay on screen.
///
/// To fix both, every non-ASCII cell is drawn separately, which forces a cursor move to the next
/// cell, and the cells covered by a wide glyph are blanked before the glyph is drawn. ASCII-only
/// content is passed through unchanged.
pub struct WidthSafeBackend<B> {
    inner: B,
    /// Last cursor position set by ratatui
    ///
    /// Reported instead of querying the terminal, which requires reading the answer from stdin
    /// while the input event reader is running.
    cursor_position: Option<Position>,
}

impl<B> WidthSafeBackend<B> {
    pub fn new(inner: B) -> Self {
        Self {
            inner,
            cursor_position: None,
        }
    }
}

impl<B: Backend> Backend for WidthSafeBackend<B> {
    type Error = B::Error;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        let is_ascii = |(_, _, cell): &(u16, u16, &Cell)| cell.symbol().is_ascii();
        let mut content = content.peekable();
        while let Some(next) = content.peek() {
            if is_ascii(next) {
                // each draw call starts with moving the cursor to its first cell
                self.inner
                    .draw(iter::from_fn(|| content.next_if(is_ascii)))?;
            } else if let Some((x, y, cell)) = content.next() {
                let width = cell.cell_width();
                if width > 1 {
                    let mut blank = cell.clone();
                    blank.set_symbol(" ");
                    let covered = (x + 1..x.saturating_add(width)).map(|x| (x, y, &blank));
                    self.inner.draw(covered)?;
                }
                self.inner.draw(iter::once((x, y, cell)))?;
            }
        }
        Ok(())
    }

    fn append_lines(&mut self, n: u16) -> Result<(), Self::Error> {
        self.inner.append_lines(n)
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        self.inner.hide_cursor()
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        self.inner.show_cursor()
    }

    fn get_cursor_position(&mut self) -> Result<Position, Self::Error> {
        match self.cursor_position {
            Some(position) => Ok(position),
            None => self.inner.get_cursor_position(),
        }
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> Result<(), Self::Error> {
        let position = position.into();
        self.inner.set_cursor_position(position)?;
        self.cursor_position = Some(position);
        Ok(())
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.inner.clear()
    }

    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), Self::Error> {
        self.inner.clear_region(clear_type)
    }

    fn size(&self) -> Result<Size, Self::Error> {
        self.inner.size()
    }

    fn window_size(&mut self) -> Result<WindowSize, Self::Error> {
        self.inner.window_size()
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.inner.flush()
    }
}

/// Allows to write escape sequences directly to the terminal
impl<B: io::Write> io::Write for WidthSafeBackend<B> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        io::Write::flush(&mut self.inner)
    }
}

#[cfg(test)]
mod tests {
    use ratatui::backend::CrosstermBackend;

    use super::*;

    fn cell(symbol: &str) -> Cell {
        let mut cell = Cell::default();
        cell.set_symbol(symbol);
        cell
    }

    fn render<B: Backend>(backend: &mut B, cells: &[(u16, u16, Cell)])
    where
        B::Error: std::fmt::Debug,
    {
        backend
            .draw(cells.iter().map(|(x, y, cell)| (*x, *y, cell)))
            .unwrap();
        backend.flush().unwrap();
    }

    fn render_safe(cells: &[(u16, u16, Cell)]) -> String {
        let mut out = Vec::new();
        render(
            &mut WidthSafeBackend::new(CrosstermBackend::new(&mut out)),
            cells,
        );
        String::from_utf8(out).unwrap()
    }

    fn render_plain(cells: &[(u16, u16, Cell)]) -> String {
        let mut out = Vec::new();
        render(&mut CrosstermBackend::new(&mut out), cells);
        String::from_utf8(out).unwrap()
    }

    /// Strips all escape sequences except cursor moves
    fn strip_sgr(s: &str) -> String {
        let mut res = String::new();
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                let mut seq = String::from(c);
                for c in chars.by_ref() {
                    seq.push(c);
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
                if seq.ends_with('H') {
                    res.push_str(&seq);
                }
            } else {
                res.push(c);
            }
        }
        res
    }

    #[test]
    fn test_ascii_is_passed_through() {
        let cells = [
            (0, 0, cell("a")),
            (1, 0, cell("b")),
            (3, 0, cell("c")),
            (0, 1, cell("d")),
        ];
        assert_eq!(render_safe(&cells), render_plain(&cells));
    }

    #[test]
    fn test_cursor_is_moved_after_non_ascii() {
        // `✌` is 1 column wide for ratatui, but 2 columns in tmux
        let cells = [(0, 0, cell("a")), (1, 0, cell("✌")), (2, 0, cell("b"))];
        assert_eq!(strip_sgr(&render_plain(&cells)), "\x1b[1;1Ha✌b");
        let out = strip_sgr(&render_safe(&cells));
        assert_eq!(out, "\x1b[1;1Ha\x1b[1;2H✌\x1b[1;3Hb");
    }

    #[test]
    fn test_covered_cells_are_blanked() {
        // `👁‍🗨` is 2 columns wide for ratatui, but 1 column in tmux
        let cells = [(0, 0, cell("👁\u{200d}🗨")), (2, 0, cell("x"))];
        let out = strip_sgr(&render_safe(&cells));
        assert_eq!(out, "\x1b[1;2H \x1b[1;1H👁\u{200d}🗨\x1b[1;3Hx");
    }

    #[test]
    fn test_cursor_position_is_cached() {
        let mut out = Vec::new();
        {
            let mut backend = WidthSafeBackend::new(CrosstermBackend::new(&mut out));
            backend.set_cursor_position((3, 4)).unwrap();
            // answered without querying the terminal
            assert_eq!(
                backend.get_cursor_position().unwrap(),
                Position { x: 3, y: 4 }
            );
            Backend::flush(&mut backend).unwrap();
        }
        assert_eq!(String::from_utf8(out).unwrap(), "\x1b[5;4H");
    }
}
