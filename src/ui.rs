use ratatui::{
    buffer::Buffer, layout::{Alignment, Constraint, Direction, Layout, Rect}, style::{Color, Modifier, Style}, text::{Line, Span}, widgets::{Axis, Chart, Dataset, GraphType, Paragraph, Widget, Wrap}
};
use unicode_width::UnicodeWidthStr;
use webbrowser::Browser;

use crate::thok::{Outcome, Thok};

const HORIZONTAL_MARGIN: u16 = 5;
const VERTICAL_MARGIN: u16 = 2;

impl Widget for &Thok {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // styles
        let bold_style = Style::default().add_modifier(Modifier::BOLD);

        let green_bold_style = Style::default().patch(bold_style).fg(Color::Green);
        let red_bold_style = Style::default().patch(bold_style).fg(Color::Red);

        let dim_bold_style = Style::default()
            .patch(bold_style)
            .add_modifier(Modifier::DIM);

        let underlined_dim_bold_style = Style::default()
            .patch(dim_bold_style)
            .add_modifier(Modifier::UNDERLINED);

        let italic_style = Style::default().add_modifier(Modifier::ITALIC);

        let magenta_style = Style::default().fg(Color::Magenta);

        if self.has_finished() {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .horizontal_margin(HORIZONTAL_MARGIN)
                .vertical_margin(VERTICAL_MARGIN)
                .constraints(
                    [
                        Constraint::Min(1),
                        Constraint::Length(1),
                        Constraint::Length(1), // for padding
                        Constraint::Length(1),
                    ]
                    .as_ref(),
                )
                .split(area);

            let mut highest_wpm = 0.0;

            for ts in &self.wpm_coords {
                if ts.1 > highest_wpm {
                    highest_wpm = ts.1;
                }
            }

            let datasets = vec![Dataset::default()
                .marker(ratatui::symbols::Marker::Braille)
                .style(magenta_style)
                .graph_type(GraphType::Line)
                .data(&self.wpm_coords)];

            let mut overall_duration = match self.wpm_coords.last() {
                Some(x) => x.0,
                _ => self.seconds_remaining.unwrap_or(1.0),
            };

            overall_duration = if overall_duration < 1.0 {
                1.0
            } else {
                overall_duration
            };

            let chart = Chart::new(datasets)
                .x_axis(
                    Axis::default()
                        .title("seconds")
                        .bounds([1.0, overall_duration])
                        .labels(vec![
                            Span::styled("1", bold_style),
                            Span::styled(format!("{:.2}", overall_duration), bold_style),
                        ]),
                )
                .y_axis(
                    Axis::default()
                        .title("wpm")
                        .bounds([0.0, highest_wpm.round()])
                        .labels(vec![
                            Span::styled("0", bold_style),
                            Span::styled(format!("{}", highest_wpm.round()), bold_style),
                        ]),
                );

            chart.render(chunks[0], buf);

            let stats = Paragraph::new(Span::styled(
                format!(
                    "{} wpm   {}% acc   {:.2} sd",
                    self.wpm, self.accuracy, self.std_dev
                ),
                bold_style,
            ))
            .alignment(Alignment::Center);

            stats.render(chunks[1], buf);

            let legend = Paragraph::new(Span::styled(
                String::from(if Browser::is_available() {
                    "(r)etry / (n)ew / (t)weet / (esc)ape"
                } else {
                    "(r)etry / (n)ew / (esc)ape"
                }),
                italic_style,
            ));

            legend.render(chunks[3], buf);
        } else {
            let max_chars_per_line = area.width - (HORIZONTAL_MARGIN * 2);
            let mut prompt_occupied_lines =
                ((self.prompt.width() as f64 / max_chars_per_line as f64).ceil() + 1.0) as u16;

            let time_left_lines = if self.number_of_secs.is_some() { 2 } else { 0 };

            let pace_position = self.pace.and_then(|p| {
                let total_chars = self.prompt.len() as f64;
                let progress = ((p / 60.0) * self.started_at?.elapsed().ok()?.as_secs_f64()) / self.number_of_words as f64;
                Some((progress * total_chars).round() as usize)
            });

            if self.prompt.width() <= max_chars_per_line as usize {
                prompt_occupied_lines = 1;
            }

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .horizontal_margin(HORIZONTAL_MARGIN)
                .constraints(
                    [
                        Constraint::Length(
                            ((area.height as f64 - prompt_occupied_lines as f64) / 2.0) as u16,
                        ),
                        Constraint::Length(time_left_lines),
                        Constraint::Length(prompt_occupied_lines),
                        Constraint::Length(
                            ((area.height as f64 - prompt_occupied_lines as f64) / 2.0) as u16,
                        ),
                    ]
                    .as_ref(),
                )
                .split(area);

            let mut past_pace_caret = false;

            let mut spans = self
                .input
                .iter()
                .enumerate()
                .map(|(idx, input)| {
                    let expected = self.get_expected_char(idx).to_string();

                    let mut display_char = match input.outcome {
                        Outcome::Incorrect => Span::styled(
                            match expected.as_str() {
                                " " => "·".to_owned(),
                                _ => expected,
                            },
                            red_bold_style,
                        ),
                        Outcome::Correct => Span::styled(expected, green_bold_style),
                    };
                    if let Some(p) = pace_position {
                        if p == idx {
                            let prev_style = display_char.style;
                            display_char = display_char.style(prev_style.bg(Color::White));
                            past_pace_caret = true;
                        }
                    }
                    display_char
                })
                .collect::<Vec<Span>>();

            spans.push(Span::styled(
                self.get_expected_char(self.cursor_pos).to_string(),
                if let Some(p) = pace_position {
                    if p == self.cursor_pos {
                        underlined_dim_bold_style.bg(Color::White)
                    } else {
                        underlined_dim_bold_style
                    }
                } else {
                    underlined_dim_bold_style
                }
            ));

            let full_span = Span::styled(
                self.prompt[(self.cursor_pos + 1)..self.prompt.len()].to_string(),
                dim_bold_style,
            );
            let next_idx = self.cursor_pos + 1;
            let len = self.prompt.len();
            let remaining = if let Some(v) = pace_position {
                if (next_idx..len).contains(&v) {
                    vec![
                        Span::styled(self.prompt[next_idx..v].to_string(), dim_bold_style),
                        Span::styled(self.get_expected_char(v).to_string(), dim_bold_style.bg(Color::White)),
                        Span::styled(self.prompt[v + 1..len].to_string(), dim_bold_style)
                    ]
                } else {
                    vec![full_span]
                }
            } else {
                vec![full_span]
            };
            spans.extend(remaining);

            let widget = Paragraph::new(Line::from(spans))
                .alignment(if prompt_occupied_lines == 1 {
                    // when the prompt is small enough to fit on one line
                    // centering the text gives a nice zen feeling
                    Alignment::Center
                } else {
                    Alignment::Left
                })
                .wrap(Wrap { trim: true });

            widget.render(chunks[2], buf);

            if self.seconds_remaining.is_some() {
                let timer = Paragraph::new(Span::styled(
                    format!("{:.1}", self.seconds_remaining.unwrap()),
                    dim_bold_style,
                ))
                .alignment(Alignment::Center);

                timer.render(chunks[1], buf);
            }
        }
    }
}
