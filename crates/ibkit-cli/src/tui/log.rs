use chrono::{DateTime, Local};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::widgets::ListState;
use std::collections::VecDeque;

/// Entries kept; the oldest go first.
pub const CAPACITY: usize = 2000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Level {
    Info,
    Good,
    Warn,
    Error,
}

pub struct Entry {
    id: u64,
    pub at: DateTime<Local>,
    pub level: Level,
    pub text: String,
    /// The raw controller event, pretty-printed, for the detail view.
    pub detail: Option<String>,
    /// Identical consecutive lines are kept once with a count.
    pub repeats: u32,
}

/// The event log: entries, the filter, and a selection that either follows
/// the newest entry or stays on one entry while new ones arrive.
#[derive(Default)]
pub struct Log {
    entries: VecDeque<Entry>,
    next: u64,
    /// The selected entry's id; `None` follows the newest entry.
    selected: Option<u64>,
    pub filter: String,
    /// Keys go to the filter text.
    pub typing: bool,
    /// Only warnings and errors.
    pub problems: bool,
    /// Scroll offset, kept between frames.
    pub list: ListState,
    /// Visible rows in the last frame, for PgUp and PgDn.
    pub page: usize,
}

impl Log {
    pub fn push(
        &mut self,
        at: DateTime<Local>,
        level: Level,
        text: String,
        detail: Option<String>,
    ) {
        if let Some(last) = self.entries.back_mut()
            && last.level == level
            && last.text == text
        {
            last.repeats += 1;
            last.at = at;
            last.detail = detail;
            return;
        }
        if self.entries.len() == CAPACITY {
            self.entries.pop_front();
        }
        self.entries.push_back(Entry {
            id: self.next,
            at,
            level,
            text,
            detail,
            repeats: 1,
        });
        self.next += 1;
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn following(&self) -> bool {
        self.selected.is_none()
    }

    /// The entries that pass the filter, oldest first.
    pub fn view(&self) -> Vec<&Entry> {
        let filter = self.filter.to_lowercase();
        self.entries
            .iter()
            .filter(|entry| {
                (!self.problems || matches!(entry.level, Level::Warn | Level::Error))
                    && (filter.is_empty() || entry.text.to_lowercase().contains(&filter))
            })
            .collect()
    }

    /// The selected row in `view`: the newest when following, else the
    /// selected entry or the next one that is still kept.
    pub fn position(&self, view: &[&Entry]) -> Option<usize> {
        let last = view.len().checked_sub(1)?;
        Some(self.selected.map_or(last, |id| {
            view.iter().position(|entry| entry.id >= id).unwrap_or(last)
        }))
    }

    pub fn selected(&self) -> Option<&Entry> {
        let view = self.view();
        self.position(&view).map(|at| view[at])
    }

    /// Stay on the selected entry, as the detail view shows it.
    pub fn pin(&mut self) {
        self.selected = self.selected().map(|entry| entry.id);
    }

    /// Handle a navigation or filter key; false when the key is not one.
    pub fn key(&mut self, key: KeyEvent) -> bool {
        if self.typing {
            match key.code {
                KeyCode::Enter => self.typing = false,
                KeyCode::Esc => {
                    self.typing = false;
                    self.filter.clear();
                }
                KeyCode::Backspace => {
                    self.filter.pop();
                }
                KeyCode::Char(ch) if !ch.is_control() && self.filter.len() < 200 => {
                    self.filter.push(ch)
                }
                _ => {}
            }
            return true;
        }
        let page = self.page.max(1) as isize;
        let step = match key.code {
            KeyCode::Up | KeyCode::Char('k') => -1,
            KeyCode::Down | KeyCode::Char('j') => 1,
            KeyCode::PageUp => -page,
            KeyCode::PageDown => page,
            KeyCode::Home | KeyCode::Char('g') => isize::MIN,
            KeyCode::End | KeyCode::Char('G') => isize::MAX,
            KeyCode::Char('/') => {
                self.typing = true;
                return true;
            }
            KeyCode::Char('e') => {
                self.problems = !self.problems;
                return true;
            }
            KeyCode::Esc if self.problems || !self.filter.is_empty() => {
                self.problems = false;
                self.filter.clear();
                return true;
            }
            _ => return false,
        };
        let view = self.view();
        if let Some(at) = self.position(&view) {
            let target = (at as isize)
                .saturating_add(step)
                .clamp(0, view.len() as isize - 1);
            // Reaching the newest entry follows new ones again.
            self.selected = (target as usize + 1 < view.len()).then(|| view[target as usize].id);
        }
        true
    }
}
