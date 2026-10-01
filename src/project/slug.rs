//! The project slug newtype and its grammar.
//!
//! A slug is the fleet's one canonical identity for a body of work. Its
//! grammar (`^[a-z0-9][a-z0-9-]*$`, at most 64 bytes) is the intersection of
//! every consumer's own storage alphabet, so a slug is storable anywhere in
//! the fleet with no further normalization.

use std::borrow::Borrow;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The maximum length of a [`Slug`] in bytes.
pub const SLUG_MAX_BYTES: usize = 64;

/// A validated project slug: `^[a-z0-9][a-z0-9-]*$`, at most
/// [`SLUG_MAX_BYTES`] bytes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Slug(String);

/// Why a candidate string is not a valid [`Slug`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SlugError {
    /// The candidate string was empty.
    #[error("a project slug cannot be empty")]
    Empty,
    /// The candidate string was longer than [`SLUG_MAX_BYTES`] bytes.
    #[error("slug {raw:?} is {len} bytes, over the {SLUG_MAX_BYTES}-byte limit")]
    TooLong {
        /// The candidate string that was too long.
        raw: String,
        /// Its length in bytes.
        len: usize,
    },
    /// The candidate string did not match `^[a-z0-9][a-z0-9-]*$`.
    #[error("slug {raw:?} does not match ^[a-z0-9][a-z0-9-]*$: {reason}")]
    InvalidGrammar {
        /// The candidate string that failed to parse.
        raw: String,
        /// A short, stable description of what was wrong.
        reason: &'static str,
    },
}

impl Slug {
    /// Validate `raw` against the slug grammar and construct a [`Slug`].
    ///
    /// # Errors
    ///
    /// Returns [`SlugError::Empty`] for an empty string,
    /// [`SlugError::TooLong`] for a string over [`SLUG_MAX_BYTES`] bytes, and
    /// [`SlugError::InvalidGrammar`] for any string containing a byte other
    /// than a lowercase ASCII letter, an ASCII digit, or a hyphen, or one
    /// that begins with a hyphen.
    pub fn new(raw: impl Into<String>) -> Result<Self, SlugError> {
        let raw = raw.into();
        if raw.is_empty() {
            return Err(SlugError::Empty);
        }
        if raw.len() > SLUG_MAX_BYTES {
            let len = raw.len();
            return Err(SlugError::TooLong { raw, len });
        }

        let mut chars = raw.chars();
        match chars.next() {
            Some(first) if first.is_ascii_lowercase() || first.is_ascii_digit() => {}
            _ => {
                return Err(SlugError::InvalidGrammar {
                    raw,
                    reason: "must start with a lowercase ASCII letter or digit",
                });
            }
        }
        if !raw
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
        {
            return Err(SlugError::InvalidGrammar {
                raw,
                reason: "must contain only lowercase ASCII letters, digits, and hyphens",
            });
        }

        Ok(Self(raw))
    }

    /// Return the slug's text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the slug and return its text.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for Slug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Borrow<str> for Slug {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for Slug {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for Slug {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::new(raw).map_err(serde::de::Error::custom)
    }
}

impl Serialize for Slug {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_the_minimal_slug() {
        assert_eq!("a", Slug::new("a").expect("valid slug").as_str());
    }

    #[test]
    fn accepts_digits_and_hyphens_after_the_first_character() {
        let slug = Slug::new("kb-2026").expect("valid slug");
        assert_eq!("kb-2026", slug.as_str());
    }

    #[test]
    fn rejects_an_empty_string() {
        assert_eq!(Err(SlugError::Empty), Slug::new(""));
    }

    #[test]
    fn rejects_a_string_over_the_byte_limit() {
        let raw = "a".repeat(SLUG_MAX_BYTES + 1);
        let err = Slug::new(raw.clone()).expect_err("too long");
        assert_eq!(
            SlugError::TooLong {
                raw,
                len: SLUG_MAX_BYTES + 1
            },
            err
        );
    }

    #[test]
    fn accepts_a_string_at_exactly_the_byte_limit() {
        let raw = "a".repeat(SLUG_MAX_BYTES);
        assert!(Slug::new(raw).is_ok());
    }

    #[test]
    fn rejects_a_leading_hyphen() {
        assert!(matches!(
            Slug::new("-abc"),
            Err(SlugError::InvalidGrammar { .. })
        ));
    }

    #[test]
    fn rejects_an_uppercase_letter() {
        assert!(matches!(
            Slug::new("Kb"),
            Err(SlugError::InvalidGrammar { .. })
        ));
    }

    #[test]
    fn rejects_a_slash() {
        assert!(matches!(
            Slug::new("host/owner/name"),
            Err(SlugError::InvalidGrammar { .. })
        ));
    }

    #[test]
    fn rejects_an_underscore() {
        assert!(matches!(
            Slug::new("kb_lib"),
            Err(SlugError::InvalidGrammar { .. })
        ));
    }

    #[test]
    fn display_renders_the_slug_text() {
        let slug = Slug::new("kb").expect("valid slug");
        assert_eq!("kb", slug.to_string());
    }

    #[test]
    fn borrow_and_as_ref_expose_the_slug_text() {
        use std::borrow::Borrow;
        let slug = Slug::new("kb").expect("valid slug");
        let borrowed: &str = slug.borrow();
        assert_eq!("kb", borrowed);
        assert_eq!("kb", slug.as_ref());
    }

    #[test]
    fn into_string_returns_owned_text() {
        let slug = Slug::new("kb").expect("valid slug");
        assert_eq!("kb".to_string(), slug.into_string());
    }

    #[test]
    fn deserializes_a_valid_slug_from_json() {
        let slug: Slug = serde_json::from_str("\"kb\"").expect("valid json");
        assert_eq!("kb", slug.as_str());
    }

    #[test]
    fn deserialize_rejects_an_invalid_slug() {
        let result: Result<Slug, _> = serde_json::from_str("\"Kb\"");
        assert!(result.is_err());
    }

    #[test]
    fn serializes_as_a_plain_string() {
        let slug = Slug::new("kb").expect("valid slug");
        assert_eq!("\"kb\"", serde_json::to_string(&slug).expect("serialize"));
    }

    #[test]
    fn slug_error_display_includes_the_raw_candidate() {
        let err = SlugError::InvalidGrammar {
            raw: "Kb".to_string(),
            reason: "must start with a lowercase ASCII letter or digit",
        };
        assert!(err.to_string().contains("Kb"));
    }
}
