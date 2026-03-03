use std::io::{self, Stderr};
use std::time::Instant;

use crossterm::execute;
use crossterm::cursor::Show;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    style::{Color, Style},
    symbols,
    text::{Line, Span},
    widgets::Paragraph,
    Terminal, TerminalOptions, Viewport,
};

struct RenderState {
    status: String,
    current: u32,
    total: u32,
    start_time: Instant,
    output_path: String,
    resolution: String,
    fps: u32,
    threads: usize,
    codec: String,
}

pub struct TuiProgress {
    terminal: Terminal<CrosstermBackend<Stderr>>,
    state: RenderState,
}

const VIEWPORT_HEIGHT: u16 = 7;

impl TuiProgress {
    pub fn new(
        total: u32,
        output_path: &str,
        width: u32,
        height: u32,
        fps: u32,
        codec: &str,
    ) -> anyhow::Result<Self> {
        let backend = CrosstermBackend::new(io::stderr());
        let terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(VIEWPORT_HEIGHT),
            },
        )?;

        let state = RenderState {
            status: "Rendering frames".to_string(),
            current: 0,
            total,
            start_time: Instant::now(),
            output_path: output_path.to_string(),
            resolution: format!("{}×{}", width, height),
            fps,
            threads: rayon::current_num_threads(),
            codec: codec.to_string(),
        };

        let mut tui = Self { terminal, state };
        tui.draw()?;
        Ok(tui)
    }

    pub fn set_status(&mut self, msg: &str) {
        self.state.status = msg.to_string();
        let _ = self.draw();
    }

    pub fn set_progress(&mut self, current: u32) {
        self.state.current = current;
        let _ = self.draw();
    }

    pub fn finish(mut self, msg: &str) {
        self.state.status = msg.to_string();
        self.state.current = self.state.total;
        let _ = self.draw();
        self.cleanup();
    }

    fn cleanup(&mut self) {
        let _ = execute!(
            self.terminal.backend_mut(),
            Show,
        );
    }

    fn draw(&mut self) -> anyhow::Result<()> {
        let state = &self.state;

        let ratio = if state.total > 0 {
            (state.current as f64 / state.total as f64).min(1.0)
        } else {
            0.0
        };
        let percent = (ratio * 100.0) as u16;

        let elapsed = state.start_time.elapsed();
        let elapsed_secs = elapsed.as_secs();
        let eta_secs = if state.current > 0 {
            let total_estimated = elapsed_secs as f64 / ratio;
            (total_estimated - elapsed_secs as f64).max(0.0) as u64
        } else {
            0
        };

        let elapsed_str = format!("{:02}:{:02}", elapsed_secs / 60, elapsed_secs % 60);
        let eta_str = format!("{:02}:{:02}", eta_secs / 60, eta_secs % 60);

        let output_path = state.output_path.clone();
        let config_line = format!(
            "{} @ {}fps · {} threads · {}",
            state.resolution, state.fps, state.threads, state.codec
        );
        let status = state.status.clone();
        let progress_label = format!("{}/{}  {}%", state.current, state.total, percent);
        let timing_line = format!("Elapsed {}  ·  ETA {}", elapsed_str, eta_str);

        self.terminal.draw(|frame| {
            let area = frame.area();

            let chunks = Layout::vertical([
                Constraint::Length(1), // Output
                Constraint::Length(1), // Config
                Constraint::Length(1), // Status
                Constraint::Length(1), // Spacer
                Constraint::Length(1), // Progress bar
                Constraint::Length(1), // Spacer
                Constraint::Length(1), // Timing
            ])
            .split(area);

            // Output line
            let output_line = Line::from(vec![
                Span::styled("Output    ", Style::default().fg(Color::DarkGray)),
                Span::raw(&output_path),
            ]);
            frame.render_widget(Paragraph::new(output_line), chunks[0]);

            // Config line
            let config = Line::from(vec![
                Span::styled("Config    ", Style::default().fg(Color::DarkGray)),
                Span::raw(&config_line),
            ]);
            frame.render_widget(Paragraph::new(config), chunks[1]);

            // Status line
            let status_line = Line::from(vec![
                Span::styled("Status    ", Style::default().fg(Color::DarkGray)),
                Span::styled(&status, Style::default().fg(Color::Green)),
            ]);
            frame.render_widget(Paragraph::new(status_line), chunks[2]);

            // Progress bar: "72/810  8% ━━━━━━━━━━━━━━━━━━━━"
            let bar_width: usize = 20;
            let filled = (ratio * bar_width as f64).round() as usize;
            let unfilled = bar_width - filled;
            let filled_str = symbols::line::THICK.horizontal.repeat(filled);
            let unfilled_str = symbols::line::THICK.horizontal.repeat(unfilled);
            let progress_line = Line::from(vec![
                Span::raw(&progress_label),
                Span::raw(" "),
                Span::styled(filled_str, Style::default().fg(Color::Cyan)),
                Span::styled(unfilled_str, Style::default().fg(Color::DarkGray)),
            ]);
            frame.render_widget(Paragraph::new(progress_line), chunks[4]);

            // Timing line
            let timing = Line::from(vec![Span::styled(
                &timing_line,
                Style::default().fg(Color::DarkGray),
            )]);
            frame.render_widget(Paragraph::new(timing), chunks[6]);
        })?;

        Ok(())
    }
}

impl Drop for TuiProgress {
    fn drop(&mut self) {
        self.cleanup();
    }
}
