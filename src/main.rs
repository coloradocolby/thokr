mod lang;
mod thok;
mod ui;
mod util;

use crate::{lang::Language, thok::Thok};
use clap::{ArgEnum, ErrorKind, IntoApp, Parser};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    tty::IsTty,
};
use std::{
    error::Error,
    io::{self, stdin},
    sync::mpsc,
    thread,
    time::Duration,
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    Frame, Terminal,
};
use webbrowser::Browser;

const TICK_RATE_MS: u64 = 100;

/// sleek typing tui with visualized results and historical logging
#[derive(Parser, Debug, Clone)]
#[clap(version, about, long_about= None)]
pub struct Cli {
    /// number of words to use in test
    #[clap(short = 'w', long, default_value_t = 15)]
    number_of_words: usize,

    /// number of sentences to use in test
    #[clap(short = 'f', long = "full-sentences")]
    number_of_sentences: Option<usize>,

    /// number of seconds to run test
    #[clap(short = 's', long)]
    number_of_secs: Option<usize>,

    /// custom prompt to use
    #[clap(short = 'p', long)]
    prompt: Option<String>,

    /// language to pull words from
    #[clap(short = 'l', long, arg_enum, default_value_t = SupportedLanguage::English)]
    supported_language: SupportedLanguage,

    /// the pace cursor's speed
    #[clap(long)]
    pace: Option<u16>,
}

#[derive(Debug, Copy, Clone, ArgEnum, strum_macros::Display)]
enum SupportedLanguage {
    English,
    English1k,
    English10k,
}

impl SupportedLanguage {
    fn as_lang(&self) -> Language {
        Language::new(self.to_string().to_lowercase())
    }
}

#[derive(Debug)]
struct App {
    cli: Option<Cli>,
    thok: Thok,
}

impl App {
    fn new(cli: Cli) -> Self {
        let mut count = 0;
        let pace = cli.pace.map(|p| p as f64);
        let prompt = if cli.prompt.is_some() {
            cli.prompt.clone().unwrap()
        } else if cli.number_of_sentences.is_some() {
            let language = cli.supported_language.as_lang();
            let (s, count_tmp) = language.get_random_sentence(cli.number_of_sentences.unwrap());
            count = count_tmp;
            // sets the word count for the sentence.
            s.join("")
        } else {
            let language = cli.supported_language.as_lang();

            language.get_random(cli.number_of_words).join(" ")
        };
        if cli.number_of_sentences.is_some() {
            Self {
                thok: Thok::new(prompt, count, cli.number_of_secs.map(|ns| ns as f64), pace),
                cli: Some(cli),
            }
        } else {
            Self {
                thok: Thok::new(
                    prompt,
                    cli.number_of_words,
                    cli.number_of_secs.map(|ns| ns as f64),
                    pace
                ),
                cli: Some(cli),
            }
        }
    }

    fn reset(&mut self, new_prompt: Option<String>) {
        let cli = self.cli.clone().unwrap();
        let mut count = 0;
        let pace = cli.pace.map(|p| p as f64);
        let prompt = match new_prompt {
            Some(_) => new_prompt.unwrap(),
            _ => match cli.number_of_sentences {
                Some(t) => {
                    let language = cli.supported_language.as_lang();
                    let (s, count_tmp) = language.get_random_sentence(t);
                    count = count_tmp;
                    // sets the word count for the sentence
                    s.join("")
                }
                _ => {
                    let language = cli.supported_language.as_lang();
                    language.get_random(cli.number_of_words).join(" ")
                }
            },
        };
        if cli.number_of_sentences.is_some() {
            self.thok = Thok::new(prompt, count, cli.number_of_secs.map(|ns| ns as f64), pace);
        } else {
            self.thok = Thok::new(
                prompt,
                cli.number_of_words,
                cli.number_of_secs.map(|ns| ns as f64),
                pace
            );
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();

    if !stdin().is_tty() {
        let mut cmd = Cli::command();
        cmd.error(ErrorKind::Io, "stdin must be a tty").exit();
    }

    enable_raw_mode()?;

    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(cli);
    start_tui(&mut terminal, &mut app)?;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen,)?;
    terminal.show_cursor()?;

    Ok(())
}

enum ExitType {
    Restart,
    New,
    Quit,
}
fn start_tui<B: Backend>(
    terminal: &mut Terminal<B>,
    mut app: &mut App,
) -> Result<(), Box<dyn Error>> {
    let cli = app.cli.clone().expect("Expected CLI");

    let should_tick = cli.number_of_secs.unwrap_or(0) > 0 || matches!(cli.pace, Some(_v));

    let thok_events = get_thok_events(should_tick);

    loop {
        let mut exit_type: ExitType = ExitType::Quit;
        terminal.draw(|f| ui(app, f))?;

        loop {
            let app = &mut app;

            match thok_events.recv()? {
                ThokEvent::Tick => {
                    if app.thok.has_started() && !app.thok.has_finished() {
                        app.thok.on_tick();

                        if app.thok.has_finished() {
                            app.thok.calc_results();
                        }
                        terminal.draw(|f| ui(app, f))?;
                    }
                }
                ThokEvent::Resize => {
                    terminal.draw(|f| ui(app, f))?;
                }
                ThokEvent::Key(key) => {
                    match key.code {
                        KeyCode::Esc => {
                            break;
                        }
                        KeyCode::Backspace => {
                            if !app.thok.has_finished() {
                                app.thok.backspace();
                            }
                        }
                        KeyCode::Left => {
                            exit_type = ExitType::Restart;
                            break;
                        }
                        KeyCode::Right => {
                            exit_type = ExitType::New;
                            break;
                        }
                        KeyCode::Char(c) => {
                            if key.modifiers.contains(KeyModifiers::CONTROL)
                                && key.code == KeyCode::Char('c')
                            // ctrl+c to quit
                            {
                                break;
                            }

                            match app.thok.has_finished() {
                                false => {
                                    app.thok.write(c);
                                    if app.thok.has_finished() {
                                        app.thok.calc_results();
                                    }
                                }
                                true => match key.code {
                                    KeyCode::Char('t') => {
                                        if Browser::is_available() {
                                            webbrowser::open(&format!("https://twitter.com/intent/tweet?text={}%20wpm%20%2F%20{}%25%20acc%20%2F%20{:.2}%20sd%0A%0Ahttps%3A%2F%2Fgithub.com%thatvegandev%2Fthokr", app.thok.wpm, app.thok.accuracy, app.thok.std_dev))
                                    .unwrap_or_default();
                                        }
                                    }
                                    KeyCode::Char('r') => {
                                        exit_type = ExitType::Restart;
                                        break;
                                    }
                                    KeyCode::Char('n') => {
                                        exit_type = ExitType::New;
                                        break;
                                    }
                                    _ => {}
                                },
                            }
                        }
                        _ => {}
                    }
                    terminal.draw(|f| ui(app, f))?;
                }
            }
        }

        match exit_type {
            ExitType::Restart => {
                app.reset(Some(app.thok.prompt.clone()));
            }
            ExitType::New => {
                app.reset(None);
            }
            ExitType::Quit => {
                break;
            }
        }
    }

    Ok(())
}

#[derive(Clone)]
enum ThokEvent {
    Key(KeyEvent),
    Resize,
    Tick,
}

fn get_thok_events(should_tick: bool) -> mpsc::Receiver<ThokEvent> {
    let (tx, rx) = mpsc::channel();

    if should_tick {
        let tick_x = tx.clone();
        thread::spawn(move || loop {
            if tick_x.send(ThokEvent::Tick).is_err() {
                break;
            }

            thread::sleep(Duration::from_millis(TICK_RATE_MS))
        });
    }

    thread::spawn(move || loop {
        let evt = match event::read().unwrap() {
            Event::Key(key) => Some(ThokEvent::Key(key)),
            Event::Resize(_, _) => Some(ThokEvent::Resize),
            _ => None,
        };

        if evt.is_some() && tx.send(evt.unwrap()).is_err() {
            break;
        }
    });

    rx
}

fn ui(app: &mut App, f: &mut Frame) {
    f.render_widget(&app.thok, f.size());
}
