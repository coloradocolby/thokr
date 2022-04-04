use crate::util::std_deviation;
use itertools::Itertools;
use log::info;
use std::{char, collections::HashMap, fmt::Error, time::SystemTime};
use tui::{
    backend::Backend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Span, Spans},
    widgets::{Axis, Chart, Dataset, GraphType, Paragraph, Wrap},
    Frame,
};
use unicode_width::UnicodeWidthStr;

const HORIZONTAL_MARGIN: u16 = 10;

#[derive(Clone, Debug, Copy, PartialEq)]
pub enum Outcome {
    Correct,
    Incorrect,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Input {
    pub char: char,
    pub outcome: Outcome,
    pub timestamp: SystemTime,
}

#[derive(Clone, Debug)]
pub struct Thok {
    pub prompt: String,
    pub input: Vec<Input>,
    pub raw_coords: Vec<(f64, f64)>,
    pub wpm_coords: Vec<(f64, f64)>,
    pub cursor_pos: usize,
    pub started_at: Option<SystemTime>,
    pub duration: Option<f64>,
    pub wpm: f64,
    pub accuracy: f64,
    pub std_dev: f64,
}

impl Thok {
    pub fn new(prompt_string: String, duration: Option<usize>) -> Self {
        let duration = match duration {
            Some(d) => Some(d as f64),
            _ => None,
        };

        Self {
            prompt: prompt_string,
            input: vec![],
            raw_coords: vec![],
            wpm_coords: vec![],
            cursor_pos: 0,
            started_at: None,
            duration,
            wpm: 0.0,
            accuracy: 0.0,
            std_dev: 0.0,
        }
    }

    pub fn on_tick(self: &mut Self) {
        info!("on_tick");
        self.duration = Some(self.duration.unwrap() - 0.1);
    }

    pub fn get_expected_char(&self, idx: usize) -> char {
        self.prompt.chars().nth(idx).unwrap()
    }

    pub fn increment_cursor(&mut self) {
        if self.cursor_pos < self.input.len() {
            self.cursor_pos += 1;
        }
    }

    pub fn decrement_cursor(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
        }
    }

    pub fn calc_results(&mut self) {
        let elapsed = self.started_at.unwrap().elapsed();

        let correct_chars = self
            .input
            .clone()
            .into_iter()
            .filter(|i| i.outcome == Outcome::Correct)
            .collect::<Vec<Input>>();

        let total_time = elapsed.unwrap().as_millis() as f64 / 1000.0;
        // TODO this causes an issue if tests takes less than 1 second
        let whole_second_limit = total_time.floor();

        let correct_chars_per_sec: Vec<(f64, f64)> = correct_chars
            .clone()
            .into_iter()
            .fold(HashMap::new(), |mut map, i| {
                let mut num_secs = i
                    .timestamp
                    .duration_since(self.started_at.unwrap())
                    .unwrap()
                    .as_millis() as f64
                    / 1000.0;

                if num_secs == 0.0 {
                    num_secs = 1.;
                } else if num_secs.ceil() <= whole_second_limit {
                    if num_secs > 0. && num_secs < 1. {
                        // this accounts for the initiated keypress at 0.000
                        num_secs = 1.;
                    } else {
                        num_secs = num_secs.clone().ceil()
                    }
                } else {
                    num_secs = total_time.clone();
                }

                *map.entry(num_secs.to_string()).or_insert(0) += 1;
                map
            })
            .into_iter()
            // .map(|(k, v)| (k.parse::<f64>().unwrap(), ((v * 60) / 5) as f64))
            .map(|(k, v)| (k.parse::<f64>().unwrap(), v as f64))
            .sorted_by(|a, b| a.partial_cmp(b).unwrap())
            .collect();

        let correct_chars_at_whole_sec_intervals = correct_chars_per_sec
            .iter()
            .enumerate()
            .filter(|&(i, _)| i < correct_chars_per_sec.len() - 1)
            .map(|(_, x)| x.1)
            .collect::<Vec<f64>>();

        self.std_dev = std_deviation(&correct_chars_at_whole_sec_intervals).unwrap();

        let mut correct_chars_pressed_until_now = 0.0;

        for x in correct_chars_per_sec.clone() {
            correct_chars_pressed_until_now += x.1;
            self.wpm_coords
                .push((x.0, ((60.00 / x.0) * correct_chars_pressed_until_now) / 5.0))
        }
        self.wpm = self.wpm_coords.last().unwrap().1.ceil();
        self.accuracy = ((correct_chars.len() as f64 / self.input.len() as f64) * 100.0).round();
    }

    pub fn backspace(&mut self) {
        if self.cursor_pos > 0 {
            self.input.remove(self.cursor_pos - 1);
            self.decrement_cursor();
        }
    }

    pub fn start(&mut self) {
        self.started_at = Some(SystemTime::now());
    }

    pub fn write(&mut self, c: char) {
        info!("write start");
        let idx = self.input.len();
        if idx == 0 && self.started_at.is_none() {
            self.start();
        }

        let outcome = if c == self.get_expected_char(idx) {
            Outcome::Correct
        } else {
            Outcome::Incorrect
        };

        self.input.insert(
            self.cursor_pos,
            Input {
                char: c,
                outcome,
                timestamp: SystemTime::now(),
            },
        );
        self.increment_cursor();
        info!("write end");
    }

    pub fn has_started(&self) -> bool {
        !self.started_at.is_none()
    }

    pub fn has_finished(&self) -> bool {
        (self.input.len() == self.prompt.len())
            || (self.duration.is_some() && self.duration.unwrap() <= 0.0)
    }

    pub fn draw_prompt<B: Backend>(&mut self, f: &mut Frame<B>) -> Result<(), Error> {
        let max_chars_per_line = f.size().width - (HORIZONTAL_MARGIN * 2);
        let mut prompt_occupied_lines =
            ((self.prompt.width() as f64 / max_chars_per_line as f64).ceil() + 1.0) as u16;
        let time_left_lines = 2;

        if self.prompt.width() <= max_chars_per_line as usize {
            prompt_occupied_lines = 1;
        }
        let h = &f.size().height;
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .horizontal_margin(HORIZONTAL_MARGIN)
            .constraints(
                [
                    Constraint::Length(((*h as f64 - prompt_occupied_lines as f64) / 2.0) as u16),
                    Constraint::Length(time_left_lines),
                    Constraint::Length(prompt_occupied_lines),
                    Constraint::Length(((*h as f64 - prompt_occupied_lines as f64) / 2.0) as u16),
                ]
                .as_ref(),
            )
            .split(f.size());

        let mut spans = vec![];

        let mut idx = 0;
        info!("The prompt is {}", self.prompt);
        loop {
            let expected_char = self
                .prompt
                .chars()
                .nth(idx)
                .expect("Unable to process char")
                // TODO: chars with accents (like pequeño) fail here
                .to_string();
            let (span, style);

            let correct_input =
                idx < self.input.len() && self.input[idx].outcome == Outcome::Correct;

            if idx == self.cursor_pos {
                if idx >= self.input.len() {
                    style = Style::default()
                        .add_modifier(Modifier::BOLD)
                        .add_modifier(Modifier::DIM)
                        .add_modifier(Modifier::UNDERLINED);
                } else {
                    if correct_input {
                        style = Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD)
                            .add_modifier(Modifier::UNDERLINED);
                    } else {
                        style = Style::default()
                            .fg(Color::Red)
                            .add_modifier(Modifier::BOLD)
                            .add_modifier(Modifier::UNDERLINED);
                    }
                }
            } else {
                if idx > self.input.len() {
                    style = Style::default()
                        .add_modifier(Modifier::DIM)
                        .add_modifier(Modifier::BOLD);
                } else {
                    if correct_input {
                        style = Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD);
                    } else {
                        style = Style::default().fg(Color::Red).add_modifier(Modifier::BOLD);
                    }
                }
            }
            span = Span::styled(expected_char, style);
            spans.push(span);

            idx += 1;

            if idx == self.prompt.len() {
                break;
            }
        }

        if prompt_occupied_lines == 1 {
            // the prompt takes up less space than the terminal window, so allow for centering
            f.render_widget(
                Paragraph::new(Spans::from(spans.clone()))
                    .alignment(Alignment::Center)
                    .wrap(Wrap { trim: true }),
                chunks[2],
            );
        } else {
            f.render_widget(
                Paragraph::new(Spans::from(spans.clone())).wrap(Wrap { trim: true }),
                chunks[2],
            );
        }

        if self.duration.is_some() {
            f.render_widget(
                Paragraph::new(Span::styled(
                    String::from(format!("{}", self.duration.unwrap().floor())),
                    Style::default()
                        .add_modifier(Modifier::DIM)
                        .add_modifier(Modifier::BOLD),
                ))
                .alignment(Alignment::Center),
                chunks[1],
            );
        }
        Ok(())
    }

    pub fn draw_results<B: Backend>(&mut self, f: &mut Frame<B>) -> Result<(), Error> {
        info!("draw_results");
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .horizontal_margin(10)
            .vertical_margin(5)
            .constraints([Constraint::Percentage(90), Constraint::Min(1)].as_ref())
            .split(f.size());

        let mut highest_wpm = 0.0;

        for ts in &self.wpm_coords {
            if ts.1 > highest_wpm {
                highest_wpm = ts.1 as f64;
            }
        }

        let datasets = vec![Dataset::default()
            .marker(tui::symbols::Marker::Braille)
            .style(Style::default().fg(Color::Magenta))
            .graph_type(GraphType::Line)
            .data(&self.wpm_coords)];

        let chart = Chart::new(datasets)
            .x_axis(
                Axis::default()
                    .title("SECONDS")
                    .style(Style::default().fg(Color::Gray))
                    .bounds([1.0, self.wpm_coords.last().unwrap().0 as f64])
                    .labels(vec![
                        Span::styled("1", Style::default().add_modifier(Modifier::BOLD)),
                        Span::styled(
                            format!("{:.2}", self.wpm_coords.last().unwrap().0 as f64),
                            Style::default().add_modifier(Modifier::BOLD),
                        ),
                    ]),
            )
            .y_axis(
                Axis::default()
                    .title("WPM")
                    .style(Style::default().fg(Color::Gray))
                    .bounds([0.0, highest_wpm.round()])
                    .labels(vec![
                        Span::styled("0", Style::default().add_modifier(Modifier::BOLD)),
                        Span::styled(
                            format!("{}", highest_wpm.round()),
                            Style::default().add_modifier(Modifier::BOLD),
                        ),
                    ]),
            );

        f.render_widget(chart, chunks[0]);

        f.render_widget(
            Paragraph::new(Span::styled(
                String::from(format!(
                    "{} WPM   {}% ACC   {:.2} SD",
                    self.wpm_coords.last().unwrap().1.ceil(),
                    self.accuracy,
                    self.std_dev
                )),
                Style::default().add_modifier(Modifier::BOLD),
            ))
            .alignment(Alignment::Center),
            chunks[1],
        );
        Ok(())
    }
}