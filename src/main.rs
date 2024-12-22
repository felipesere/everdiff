#![allow(unused)]
use std::io;

use clap::{Parser, ValueEnum};
use config::config_from_env;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use diff::{Difference, Path};
use multidoc::{AdditionalDoc, DocDifference, MissingDoc};
use notify::{RecursiveMode, Watcher};
use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::prelude::StatefulWidget;
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
use tui_widget_list::{ListBuilder, ListState, ListView};

mod config;
mod diff;
mod identifier;
mod multidoc;
mod prepatch;

#[derive(Default, ValueEnum, Clone, Debug)]
enum Comparison {
    #[default]
    Index,
    Kubernetes,
}

/// Differnece between YAML documents
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Use Kubernetes comparison
    #[arg(short = 'k', long, default_value = "false")]
    kubernetes: bool,

    /// Watch the `left` and `right` files for changes and re-run
    #[arg(short = 'w', long, default_value = "false")]
    watch: bool,

    #[clap(short, long, value_delimiter = ' ', num_args = 1..)]
    left: Vec<camino::Utf8PathBuf>,
    #[clap(short, long, value_delimiter = ' ', num_args = 1..)]
    right: Vec<camino::Utf8PathBuf>,
}

#[derive(Default)]
pub struct App {
    exit: bool,
    state: ListState,
}

impl App {
    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        //terminal.draw(|frame| self.draw(frame))?;
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
    }
}

struct DifferenceState {
    difference: Difference,
}

impl Widget for DifferenceState {
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Length(2), Constraint::Length(18)])
            .split(area);

        let no_bottom_border = Block::new().borders(Borders::LEFT | Borders::TOP | Borders::RIGHT);

        Paragraph::new(self.difference.path().jq_like())
            .block(no_bottom_border)
            .render(layout[0], buf);

        let half_and_half = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![Constraint::Percentage(50), Constraint::Percentage(50)]);

        let value_areas = half_and_half.split(layout[1]);

        // left area has a left-leaning T on the top-left and no right border
        let left_area_border_set = symbols::border::Set {
            top_left: symbols::line::NORMAL.vertical_right,
            ..symbols::border::PLAIN
        };
        let left_aread_block = Block::new()
            .border_set(left_area_border_set)
            // don't render the bottom border because it will be rendered by the bottom block
            .borders(Borders::TOP | Borders::LEFT | Borders::BOTTOM)
            .title("Left")
            .title_alignment(Alignment::Center);

        Paragraph::new("Some...left..value")
            .alignment(Alignment::Left)
            .block(left_aread_block)
            .render(value_areas[0], buf);

        // the right area is super special:
        // * top-left is a T
        // * bottom-left is a flipped T
        // * top-right is a right-leaning T
        //
        let right_area_border_set = symbols::border::Set {
            top_left: symbols::line::NORMAL.horizontal_down,
            bottom_left: symbols::line::NORMAL.horizontal_up,
            top_right: symbols::line::NORMAL.vertical_left,
            ..symbols::border::PLAIN
        };
        let right_aread_block = Block::new()
            .border_set(right_area_border_set)
            // don't render the bottom border because it will be rendered by the bottom block
            .borders(Borders::ALL)
            .title("Right")
            .title_alignment(Alignment::Center);

        Paragraph::new("Right...values...like")
            .alignment(Alignment::Left)
            .block(right_aread_block)
            .render(value_areas[1], buf);
    }
}

fn fake_added_diff() -> Difference {
    let path = Path::default().push("foo").push("bar").push(1).push("baz");

    let value = serde_yaml::from_str(indoc::indoc! {r#"
            ports:
              - port: 8080
              - port: 9090
        "#})
    .unwrap();

    Difference::Added { path, value }
}

fn fake_removed_diff() -> Difference {
    let path = Path::default().push("foo").push("bar");

    let value = serde_yaml::from_str(indoc::indoc! {r#"
            bla:
              other: thing
              wheels: 6
        "#})
    .unwrap();

    Difference::Removed { path, value }
}

impl Widget for &mut App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let differences = vec![fake_added_diff(), fake_removed_diff()];

        let builder = ListBuilder::new(move |context| {
            let idx = context.index;
            let main_axis_size = 10;

            let item = differences[idx].clone();
            let s = DifferenceState { difference: item };

            (s, main_axis_size)
        });

        let item_count = 2;
        let list = ListView::new(builder, item_count);
        let state = &mut self.state;

        list.render(area, buf, state);
    }
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let mut terminal = ratatui::init();
    let app_result = App::default().run(&mut terminal);
    ratatui::restore();
    dbg!(app_result);

    // let maybe_config = config_from_env();
    // let patches = maybe_config.map(|c| c.prepatches).unwrap_or_default();

    // let left = read_and_patch(&args.left, &patches)?;
    // let right = read_and_patch(&args.right, &patches)?;

    // let comparator = if args.kubernetes {
    //     Comparison::Kubernetes
    // } else {
    //     Comparison::Index
    // };

    // let id = match comparator {
    //     Comparison::Index => identifier::by_index(),
    //     Comparison::Kubernetes => identifier::kubernetes::gvk(),
    // };

    // let ctx = multidoc::Context::new_with_doc_identifier(id);

    // let diffs = multidoc::diff(&ctx, &left, &right);

    // render_multidoc_diff(diffs);

    // if args.watch {
    //     let (tx, rx) = std::sync::mpsc::channel();

    //     let mut watcher = notify::recommended_watcher(tx)?;
    //     for p in args.left.clone().into_iter().chain(args.right.clone()) {
    //         watcher.watch(p.as_std_path(), RecursiveMode::NonRecursive)?;
    //     }

    //     for event in rx {
    //         let _event = event?;
    //         print!("{esc}[2J{esc}[1;1H", esc = 27 as char);
    //         let left = read_and_patch(&args.left, &patches)?;
    //         let right = read_and_patch(&args.right, &patches)?;

    //         let diffs = multidoc::diff(&ctx, &left, &right);

    //         render_multidoc_diff(diffs);
    //     }
    // }

    Ok(())
}

fn read_and_patch(
    paths: &[camino::Utf8PathBuf],
    patches: &[prepatch::PrePatch],
) -> anyhow::Result<Vec<serde_yaml::Value>> {
    use serde::Deserialize;

    let mut docs = Vec::new();
    for p in paths {
        let f = std::fs::File::open(p)?;
        for document in serde_yaml::Deserializer::from_reader(f) {
            let v = serde_yaml::Value::deserialize(document)?;
            docs.push(v);
        }
    }
    for patch in patches {
        let _err = patch.apply_to(&mut docs);
    }

    Ok(docs)
}

pub fn render_multidoc_diff(differences: Vec<DocDifference>) {
    use owo_colors::OwoColorize;

    if differences.is_empty() {
        println!("No differences found")
    }

    for d in differences {
        match d {
            DocDifference::Addition(AdditionalDoc { key, .. }) => {
                let key = indent::indent_all_by(4, key.to_string());
                println!("{m}", m = "Additional document:".green());
                println!("{key}");
            }
            DocDifference::Missing(MissingDoc { key, .. }) => {
                let key = indent::indent_all_by(4, key.to_string());
                println!("{m}", m = "Missing document:".red());
                println!("{key}");
            }
            DocDifference::Changed {
                key, differences, ..
            } => {
                let key = indent::indent_all_by(4, key.to_string());
                println!("Changed document:");
                println!("{key}");
                render(differences);
            }
        }
    }
}

pub fn render(differences: Vec<Difference>) {
    use owo_colors::OwoColorize;
    for d in differences {
        match d {
            Difference::Added { path, value } => {
                println!("Added: {p}:", p = path.jq_like().bold());
                let added_yaml = indent::indent_all_by(4, serde_yaml::to_string(&value).unwrap());

                println!("{a}", a = added_yaml.green());
            }
            Difference::Removed { path, value } => {
                println!("Removed: {p}:", p = path.jq_like().bold());
                let removed_yaml = indent::indent_all_by(4, serde_yaml::to_string(&value).unwrap());
                println!("{r}", r = removed_yaml.red());
            }
            Difference::Changed { path, left, right } => {
                println!("Changed: {p}:", p = path.jq_like().bold());
                let left = indent::indent_all_by(4, serde_yaml::to_string(&left).unwrap());
                let right = indent::indent_all_by(4, serde_yaml::to_string(&right).unwrap());

                print!("{r}", r = left.green());
                print!("{r}", r = right.red());
            }
        }
        println!()
    }
}
