//! Shared progress indicator helpers.

use std::time::Duration;

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

use crate::output::stderr_is_tty;

/// Build a stderr spinner when interactive progress is enabled.
#[must_use]
pub fn make_spinner(enabled: bool, message: &str) -> Option<ProgressBar> {
    if !enabled || !stderr_is_tty() {
        return None;
    }

    let progress_bar = ProgressBar::with_draw_target(None, ProgressDrawTarget::stderr());
    let style = ProgressStyle::default_spinner()
        .template("{spinner:.green} {msg}")
        .unwrap_or_else(|_| ProgressStyle::default_spinner());
    progress_bar.set_style(style);
    progress_bar.set_message(message.to_owned());
    progress_bar.enable_steady_tick(Duration::from_millis(120));

    Some(progress_bar)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn make_spinner_returns_none_when_disabled() {
        assert!(make_spinner(false, "x").is_none());
    }

    #[test]
    fn make_spinner_returns_none_without_a_tty() {
        // Under the test harness stderr is not a terminal, so an enabled
        // spinner is still suppressed. The interactive construction body
        // (lines that build and tick the ProgressBar) requires a real TTY
        // and is left to the accepted TTY-gated coverage residual.
        assert!(make_spinner(true, "working").is_none());
    }
}
