//! License display module.
//!
//! This module provides standardized license information display for common open source licenses.

use crate::output;

/// Supported license types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LicenseType {
    /// MIT License
    MIT,
    /// Apache License 2.0
    Apache2,
    /// Creative Commons CC0 1.0 Universal
    CC0,
}

impl LicenseType {
    /// Parse a license type from a string.
    ///
    /// Recognizes common variations like "MIT", "Apache-2.0", "CC0-1.0", etc.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "MIT" => Some(Self::MIT),
            "APACHE-2.0" | "APACHE2" | "APACHE" => Some(Self::Apache2),
            "CC0-1.0" | "CC0" => Some(Self::CC0),
            _ => None,
        }
    }

    /// Get the license name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::MIT => "MIT",
            Self::Apache2 => "Apache-2.0",
            Self::CC0 => "CC0-1.0",
        }
    }
}

/// Display license information for a tool.
///
/// # Arguments
/// * `tool_name` - Name of the tool
/// * `license` - License type
///
/// # Returns
/// Formatted license information string
#[must_use]
pub fn display_license(tool_name: &str, license: LicenseType) -> String {
    let mut output = format!("{tool_name} is licensed under {}\n\n", license.name());

    match license {
        LicenseType::MIT => {
            output.push_str("MIT License - A permissive license that allows:\n");
            output.push_str("• Commercial use\n");
            output.push_str("• Modification\n");
            output.push_str("• Distribution\n");
            output.push_str("• Private use\n");
            output.push('\n');
            output.push_str("Requires:\n");
            output.push_str("• License and copyright notice\n");
            output.push('\n');
            output.push_str("MIT License\n");
            output.push('\n');
            output.push_str(
                "Permission is hereby granted, free of charge, to any person obtaining a copy\n",
            );
            output.push_str(
                "of this software and associated documentation files (the \"Software\"), to deal\n",
            );
            output.push_str(
                "in the Software without restriction, including without limitation the rights\n",
            );
            output.push_str(
                "to use, copy, modify, merge, publish, distribute, sublicense, and/or sell\n",
            );
            output.push_str(
                "copies of the Software, and to permit persons to whom the Software is\n",
            );
            output.push_str("furnished to do so, subject to the following conditions:\n");
            output.push('\n');
            output.push_str(
                "The above copyright notice and this permission notice shall be included in all\n",
            );
            output.push_str("copies or substantial portions of the Software.\n");
            output.push('\n');
            output.push_str(
                "THE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR\n",
            );
            output.push_str(
                "IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,\n",
            );
            output.push_str(
                "FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE\n",
            );
            output.push_str(
                "AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER\n",
            );
            output.push_str(
                "LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,\n",
            );
            output.push_str(
                "OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE\n",
            );
            output.push_str("SOFTWARE.\n");
        }
        LicenseType::Apache2 => {
            output.push_str("Apache License 2.0 - A permissive license that allows:\n");
            output.push_str("• Commercial use\n");
            output.push_str("• Modification\n");
            output.push_str("• Distribution\n");
            output.push_str("• Patent use\n");
            output.push_str("• Private use\n");
            output.push('\n');
            output.push_str("Requires:\n");
            output.push_str("• License and copyright notice\n");
            output.push_str("• State changes\n");
        }
        LicenseType::CC0 => {
            output.push_str("Creative Commons CC0 1.0 Universal - Public domain dedication:\n");
            output.push_str("• No rights reserved\n");
            output.push_str("• Can be used for any purpose\n");
            output.push_str("• No attribution required\n");
        }
    }

    output.push('\n');

    if output::is_tty() {
        use colored::Colorize;
        use std::fmt::Write as _;
        writeln!(
            &mut output,
            "For full license text, see: {}",
            "LICENSE file in project root".blue().underline()
        )
        .unwrap_or_default();
    } else {
        output.push_str("For full license text, see: LICENSE file in project root\n");
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_license_type_from_str() {
        assert_eq!(LicenseType::parse("MIT"), Some(LicenseType::MIT));
        assert_eq!(LicenseType::parse("mit"), Some(LicenseType::MIT));
        assert_eq!(LicenseType::parse("Apache-2.0"), Some(LicenseType::Apache2));
        assert_eq!(LicenseType::parse("apache"), Some(LicenseType::Apache2));
        assert_eq!(LicenseType::parse("CC0-1.0"), Some(LicenseType::CC0));
        assert_eq!(LicenseType::parse("cc0"), Some(LicenseType::CC0));
        assert_eq!(LicenseType::parse("unknown"), None);
    }

    #[test]
    fn test_license_type_name() {
        assert_eq!(LicenseType::MIT.name(), "MIT");
        assert_eq!(LicenseType::Apache2.name(), "Apache-2.0");
        assert_eq!(LicenseType::CC0.name(), "CC0-1.0");
    }

    #[test]
    fn test_display_license_mit() {
        let output = display_license("test-tool", LicenseType::MIT);
        assert!(output.contains("test-tool"));
        assert!(output.contains("MIT"));
        assert!(output.contains("Permission is hereby granted"));
        assert!(output.contains("Commercial use"));
    }

    #[test]
    fn test_display_license_apache() {
        let output = display_license("test-tool", LicenseType::Apache2);
        assert!(output.contains("test-tool"));
        assert!(output.contains("Apache"));
        assert!(output.contains("Patent use"));
    }

    #[test]
    fn test_display_license_cc0() {
        let output = display_license("test-tool", LicenseType::CC0);
        assert!(output.contains("test-tool"));
        assert!(output.contains("CC0"));
        assert!(output.contains("No rights reserved"));
    }
}
