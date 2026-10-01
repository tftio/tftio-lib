//! Output utilities for consistent terminal formatting.

use is_terminal::IsTerminal;
use std::io;

/// Check if stdout is a TTY (terminal).
///
/// Returns `true` if stdout is connected to a terminal, `false` if piped/redirected.
/// This is used to determine whether to use colored output and fancy formatting.
#[must_use]
pub fn is_tty() -> bool {
    io::stdout().is_terminal()
}

/// Check if stderr is a TTY (terminal).
#[must_use]
pub fn stderr_is_tty() -> bool {
    io::stderr().is_terminal()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_tty_returns_bool() {
        // Just verify it returns a boolean without panicking
        let _result = is_tty();
        // Function executed successfully if we get here
    }

    #[test]
    fn test_stderr_is_tty_returns_bool() {
        let _result = stderr_is_tty();
    }
}
