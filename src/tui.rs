use crossterm::event::{self, Event, KeyCode, KeyEventKind};

use ratatui::layout::{Alignment, Constraint, Direction, Layout, Margin};
use ratatui::prelude::StatefulWidget;
use ratatui::style::Color;
use ratatui::symbols;
use ratatui::widgets::{BorderType, Borders};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Style, Stylize},
    symbols::border,
    text::{Line, Text},
    widgets::{Block, List, ListDirection, Paragraph, Widget},
    DefaultTerminal, Frame,
};
use std::io;

use tui_widget_list::{ListBuilder, ListState, ListView};

use crate::diff::{Difference, Path};
use crate::multidoc::DocDifference;
pub struct TuiApp {
    exit: bool,
    diffs: Vec<DocDifference>,
    state: MultilistState,
}

struct State {
    list: ListState,
    elements: usize,
}

struct MultilistState {
    document_state: State,
    within_doc_state: Vec<State>,
}

impl MultilistState {
    pub fn derive_from(diffs: &[DocDifference]) -> Self {
        MultilistState {
            document_state: State {
                list: ListState::default(),
                elements: diffs.len(),
            },
            within_doc_state: diffs
                .iter()
                .map(|diff| State {
                    list: ListState::default(),
                    elements: match diff {
                        DocDifference::Addition(_) => 1,
                        DocDifference::Missing(_) => 1,
                        DocDifference::Changed { differences, .. } => differences.len(),
                    },
                })
                .collect(),
        }
    }

    pub fn selected_document(&self) -> Option<usize> {
        self.document_state.list.selected
    }

    pub fn selected_change_in_doc(&self) -> Option<usize> {
        self.selected_document()
            .and_then(|idx| self.within_doc_state[idx].list.selected)
    }

    pub fn next(&mut self) {
        let doc_idx = match self.document_state.list.selected {
            Some(n) => n,
            None => {
                self.document_state.list.select(Some(0));
                0
            }
        };
        let inner_doc_state = &mut self.within_doc_state[doc_idx];
        match inner_doc_state.list.selected {
            Some(n) if n == inner_doc_state.elements => {
                // We are done with the current document. Advance the doc and select the first item
                self.document_state.list.next();
                let idx = self.document_state.list.selected.unwrap(); // WARN: Pretty sure this is safe?
                self.within_doc_state[idx].list.select(Some(0));
            }
            Some(_n) => {
                // We can still advance in the current document
                inner_doc_state.list.next();
            }
            None => {
                self.document_state.list.select(Some(0));
                self.within_doc_state[0].list.select(Some(0));
            }
        }
    }

    pub fn previous(&mut self) {
        let doc_idx = match self.document_state.list.selected {
            Some(n) => n,
            None => {
                self.document_state.list.select(Some(0));
                0
            }
        };
        let inner_doc_state = &mut self.within_doc_state[doc_idx];
        match inner_doc_state.list.selected {
            Some(0) => {
                // We are done with the current document. Go to the previous one
                self.document_state.list.previous();
                let idx = self.document_state.list.selected.unwrap(); // WARN: Pretty sure this is safe?
                let within_doc = &mut self.within_doc_state[idx];
                let last = within_doc.elements - 1;
                within_doc.list.select(Some(last));
            }
            Some(_n) => {
                // We can still advance in the current document
                inner_doc_state.list.previous();
            }
            None => {
                self.document_state.list.select(Some(0));
                self.within_doc_state[0].list.select(Some(0));
            }
        }
    }
}

impl TuiApp {
    pub fn new(diffs: Vec<DocDifference>) -> Self {
        Self {
            exit: false,
            state: MultilistState::derive_from(&diffs),
            diffs,
        }
    }

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        while !self.exit {
            terminal.draw(|frame| self.draw(frame))?;
            self.handle_events()?;
        }
        Ok(())
    }
    fn draw(&mut self, frame: &mut Frame) {
        frame.render_widget(self, frame.area())
    }

    fn handle_events(&mut self) -> io::Result<()> {
        match event::read()? {
            // it's important to check that the event is a key press event as
            // crossterm also emits key release and repeat events on Windows.
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                self.handle_key_event(key_event)
            }
            _ => {}
        };

        Ok(())
    }

    fn handle_key_event(&mut self, key_event: event::KeyEvent) {
        if key_event.code == KeyCode::Esc || key_event.code == KeyCode::Char('q') {
            self.exit = true;
        }
        if key_event.code == KeyCode::Down || key_event.code == KeyCode::Char('j') {
            self.state.next();
        }
        if key_event.code == KeyCode::Up || key_event.code == KeyCode::Char('k') {
            self.state.previous();
        }
    }
}

impl Widget for &mut TuiApp {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let differences = self.diffs.clone();
        let item_count = differences.len();

        let builder = ListBuilder::new(|context| {
            let idx = context.index;
            let main_axis_size = differences[idx].estimate_height();
            // let state = self.state.within_doc_state[idx];

            let diff = differences[idx].clone();
            let s = AllDifferencesInDocument {
                diff,
                selected: context.is_selected,
                state: ListState::default(),
            };

            (s, main_axis_size)
        });

        let list = ListView::new(builder, item_count).infinite_scrolling(true);
        let state = &mut self.state.document_state.list;

        list.render(area, buf, state);
    }
}

struct DifferenceWidget {
    difference: Difference,
}

pub fn estimate_height(diff: &Difference) -> usize {
    match diff {
        Difference::Added { value, .. } => serde_yaml::to_string(value).unwrap().lines().count(),
        Difference::Removed { value, .. } => serde_yaml::to_string(value).unwrap().lines().count(),
        Difference::Changed { left, right, .. } => {
            let left = serde_yaml::to_string(left).unwrap().lines().count();
            let right = serde_yaml::to_string(right).unwrap().lines().count();
            std::cmp::max(left, right)
        }
    }
}

impl Widget for DifferenceWidget {
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Length(2), Constraint::Length(18)])
            .split(area);

        let no_bottom_border = Block::new()
            .borders(Borders::LEFT | Borders::TOP | Borders::RIGHT)
            .border_type(BorderType::Thick);

        Paragraph::new(self.difference.path().jq_like())
            .block(no_bottom_border)
            .render(layout[0], buf);

        let half_and_half = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![Constraint::Percentage(50), Constraint::Percentage(50)]);

        let value_areas = half_and_half.split(layout[1]);

        // left area has a left-leaning T on the top-left and no right border
        let left_area_border_set = symbols::border::Set {
            top_left: symbols::line::THICK.vertical_right,
            ..symbols::border::THICK
        };
        let left_aread_block = Block::new()
            .border_set(left_area_border_set)
            // don't render the bottom border because it will be rendered by the bottom block
            .borders(Borders::TOP | Borders::LEFT | Borders::BOTTOM)
            .title("Left")
            .title_alignment(Alignment::Center);

        let left_value = match &self.difference {
            Difference::Added { .. } => Text::raw(""),
            Difference::Removed { value, .. } => {
                let raw_yaml = serde_yaml::to_string(value).unwrap();
                Text::styled(raw_yaml, Style::new().bg(Color::Red))
            }
            Difference::Changed { left, .. } => {
                let raw_yaml = serde_yaml::to_string(left).unwrap();
                Text::styled(raw_yaml, Style::new().bg(Color::Yellow).fg(Color::Black))
            }
        };

        Paragraph::new(left_value)
            .alignment(Alignment::Left)
            .block(left_aread_block)
            .render(value_areas[0], buf);

        // the right area is super special:
        // * top-left is a T
        // * bottom-left is a flipped T
        // * top-right is a right-leaning T
        //
        let right_area_border_set = symbols::border::Set {
            top_left: symbols::line::THICK.horizontal_down,
            bottom_left: symbols::line::THICK.horizontal_up,
            top_right: symbols::line::THICK.vertical_left,
            ..symbols::border::THICK
        };
        let right_aread_block = Block::new()
            .border_set(right_area_border_set)
            // don't render the bottom border because it will be rendered by the bottom block
            .borders(Borders::ALL)
            .title("Right")
            .title_alignment(Alignment::Center);

        let right_value = match &self.difference {
            Difference::Added { value, .. } => {
                let raw_yaml = serde_yaml::to_string(value).unwrap();
                Text::styled(raw_yaml, Style::new().bg(Color::Green))
            }
            Difference::Removed { value, .. } => Text::raw(""),
            Difference::Changed { right, .. } => {
                let raw_yaml = serde_yaml::to_string(right).unwrap();
                Text::styled(raw_yaml, Style::new().bg(Color::Yellow).fg(Color::Black))
            }
        };

        Paragraph::new(right_value)
            .alignment(Alignment::Left)
            .block(right_aread_block)
            .render(value_areas[1], buf);
    }
}

struct MultipleDifferencesState {
    differences: Vec<Difference>,
    state: ListState,
}

impl Widget for MultipleDifferencesState {
    fn render(mut self, area: Rect, buf: &mut Buffer) {
        let differences = self.differences.clone();
        let item_count = differences.len();

        let builder = ListBuilder::new(move |context| {
            let idx = context.index;
            let main_axis_size = 4 + estimate_height(&differences[idx]) as u16;

            let item = differences[idx].clone();
            let s = DifferenceWidget { difference: item };

            (s, main_axis_size)
        });

        let list = ListView::new(builder, item_count);
        let state = &mut self.state;

        list.render(area, buf, state);
    }
}

struct AllDifferencesInDocument {
    diff: DocDifference,
    selected: bool,
    state: ListState,
}

impl Widget for AllDifferencesInDocument {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let raw_key = self.diff.key().to_string();
        let nr_of_lines = raw_key.lines().count() as u16;

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![
                Constraint::Length(nr_of_lines),
                Constraint::Length(20),
            ])
            .split(area);

        let color = if self.selected {
            Color::Blue
        } else {
            Color::White
        };

        let title = match self.diff {
            DocDifference::Addition(_) => "Added",
            DocDifference::Missing(_) => "Missing",
            DocDifference::Changed { .. } => "Changed",
        };

        let no_bottom_border = Block::new()
            .title(title)
            .borders(Borders::LEFT | Borders::TOP | Borders::RIGHT)
            .border_style(Style::new().fg(color))
            .border_type(BorderType::Thick);

        match self.diff {
            DocDifference::Addition(_) => {
                Paragraph::new(raw_key)
                    .block(no_bottom_border)
                    .render(layout[0], buf);
            }
            DocDifference::Missing(_) => {
                Paragraph::new(raw_key)
                    .block(no_bottom_border)
                    .render(layout[0], buf);
            }
            DocDifference::Changed { differences, .. } => {
                Paragraph::new(raw_key)
                    .block(no_bottom_border)
                    .render(layout[0], buf);

                let b = Block::new()
                    .borders(Borders::LEFT | Borders::BOTTOM | Borders::RIGHT)
                    .border_type(BorderType::Thick);

                b.render(layout[1], buf);

                let inner = layout[1].inner(Margin::new(1, 1));

                let w = MultipleDifferencesState {
                    differences,
                    state: ListState::default(),
                };
                w.render(inner, buf)
            }
        }
    }
}

struct MultipleDocDifferencesState {
    differences: Vec<DocDifference>,
    states_within_doc: Vec<ListState>,
    state: ListState,
}

impl Widget for MultipleDocDifferencesState {
    fn render(mut self, area: Rect, buf: &mut Buffer) {
        let differences = self.differences.clone();
        let item_count = differences.len();

        let builder = ListBuilder::new(|context| {
            // Each item here is a single Document with possibly many differences inside
            let idx = context.index;
            let main_axis_size = differences[idx].estimate_height();
            let state = self.states_within_doc[idx].clone();

            let diff = differences[idx].clone();
            let s = AllDifferencesInDocument {
                diff,
                selected: context.is_selected,
                state,
            };

            (s, main_axis_size)
        });

        let list = ListView::new(builder, item_count).infinite_scrolling(true);
        let state = &mut self.state;

        list.render(area, buf, state);
    }
}
