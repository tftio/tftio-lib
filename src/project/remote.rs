//! Remote URL normalization.
//!
//! [`normalize_remote`] reduces every spelling of a git remote URL this
//! fleet is likely to see — the scp-like shorthand, `ssh://`, `https://`,
//! `http://`, `git://`, `file://`, and a bare filesystem path — to one
//! canonical `<host>/<path>` form: scheme, user, port, and a trailing
//! `.git` suffix are stripped, the host is lowercased, and the path's case
//! is preserved.
//!
//! A `file://` URL and an absolute or explicitly relative filesystem path
//! (one starting with `/`, `./`, or `../`) have no real host, so both
//! normalize under the pseudo-host `local`: `/home/user/kb` and
//! `file:///home/user/kb` both normalize to `local/home/user/kb`. This is a
//! documented convention of this module, not a discovered git behavior.
//!
//! A scheme-less string with no such filesystem marker — including the
//! `<host>/<path>` text this function itself produces — is instead read
//! directly as `<host>/<path>`: its first slash-separated segment becomes
//! the host. This is what makes normalization idempotent, which the
//! registry's own [`crate::project::Registry::validate`] depends on: a
//! remote already stored in normalized form must normalize to itself.
//!
//! No variant of [`RemoteError`] carries any part of the input string, so a
//! credential embedded in a malformed remote (`https://user:token@host`
//! with no path, say) cannot leak through an error message; a well-formed
//! credentialed URL is accepted and its userinfo is discarded before the
//! result is built.
//!
//! Userinfo stripping is scoped to the authority — the text before the
//! first `/` — and never to the path, and it cuts at the authority's
//! *last* `@` rather than its first. Both choices matter: a path may
//! legitimately contain `@` (`https://host/owner/repo@v1`, a tag-qualified
//! ref) and must not be mistaken for a second userinfo separator, and a
//! password may itself contain `@` (`https://u:p@ss@host/x`), in which
//! case only the text after the last `@` in the authority is the host.

use std::fmt;

/// The pseudo-host used for a `file://` URL or a bare filesystem path,
/// neither of which names a real remote host.
const LOCAL_HOST: &str = "local";

/// A remote URL reduced to `<host>/<path>`: scheme, user, port, and a
/// trailing `.git` suffix removed, host lowercased, path case preserved.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NormalizedRemote(String);

/// Why a candidate string could not be normalized.
///
/// No variant carries any part of the candidate string, so that a
/// credential embedded in a malformed remote never appears in an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RemoteError {
    /// The candidate string was empty.
    #[error("remote URL is empty")]
    Empty,
    /// No host could be identified.
    #[error("remote URL has no host")]
    MissingHost,
    /// No path was found after the host.
    #[error("remote URL has no path after the host")]
    MissingPath,
}

impl NormalizedRemote {
    /// Return the normalized `<host>/<path>` text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the normalized remote and return its text.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for NormalizedRemote {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Normalize a git remote URL to `<host>/<path>`.
///
/// # Errors
///
/// Returns [`RemoteError`] when `raw` is empty, or when a host or a path
/// segment cannot be identified in it.
pub fn normalize_remote(raw: &str) -> Result<NormalizedRemote, RemoteError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(RemoteError::Empty);
    }

    if let Some(rest) = trimmed.strip_prefix("ssh://") {
        return normalize_url_like(rest);
    }
    if let Some(rest) = trimmed.strip_prefix("https://") {
        return normalize_url_like(rest);
    }
    if let Some(rest) = trimmed.strip_prefix("http://") {
        return normalize_url_like(rest);
    }
    if let Some(rest) = trimmed.strip_prefix("git://") {
        return normalize_url_like(rest);
    }
    if let Some(rest) = trimmed.strip_prefix("file://") {
        return build(LOCAL_HOST, strip_scheme_less_userinfo(rest));
    }

    // No scheme: strip any userinfo from the authority (the text before
    // the first `/`) before scp-like detection or the host/path fallback,
    // so a credential can never survive into either.
    let without_userinfo = strip_scheme_less_userinfo(trimmed);

    if let Some((host, path)) = parse_scp_like(without_userinfo) {
        return build(host, path);
    }
    if is_filesystem_path(without_userinfo) {
        let without_leading_current = without_userinfo
            .strip_prefix("./")
            .unwrap_or(without_userinfo);
        return build(LOCAL_HOST, without_leading_current);
    }
    // No scheme, no scp-like shorthand, and no filesystem marker: read
    // directly as `<host>/<path>` text, which makes this branch (and so
    // the whole function) idempotent over its own output.
    let (host, path) = without_userinfo
        .split_once('/')
        .ok_or(RemoteError::MissingPath)?;
    build(host, path)
}

/// Whether `input` names a filesystem path rather than `<host>/<path>`
/// remote text: absolute (`/...`) or explicitly relative (`./...`,
/// `../...`).
fn is_filesystem_path(input: &str) -> bool {
    input.starts_with('/') || input.starts_with("./") || input.starts_with("../")
}

/// Strip a `user[:token]@` prefix from `authority`, cutting at its *last*
/// `@` so a `@` embedded in a password does not leave a fragment of it in
/// the host. `authority` must already exclude the path: this function does
/// not know where a path starts and must never be handed one.
fn strip_userinfo(authority: &str) -> &str {
    authority
        .rsplit_once('@')
        .map_or(authority, |(_userinfo, host)| host)
}

/// Strip a `user[:token]@` prefix from scheme-less `input`, scanning only
/// the region before the first `/` (the authority) and never the path.
///
/// Unlike [`strip_userinfo`], this takes the *whole* scheme-less remote
/// text and returns the whole remainder (authority plus path) with just
/// the authority's userinfo removed, since the caller has not yet split
/// the two apart.
fn strip_scheme_less_userinfo(input: &str) -> &str {
    let authority_end = input.find('/').unwrap_or(input.len());
    let (authority, _path_onward) = input.split_at(authority_end);
    authority
        .rfind('@')
        .map_or(input, |at| input.split_at(at + 1).1)
}

/// Normalize the part of a URL after its `scheme://`: an optional user, a
/// host with an optional port, then a slash-separated path.
fn normalize_url_like(rest: &str) -> Result<NormalizedRemote, RemoteError> {
    let (authority, path) = rest.split_once('/').ok_or(RemoteError::MissingPath)?;
    let host_port = strip_userinfo(authority);
    let host = host_port
        .split_once(':')
        .map_or(host_port, |(host, _port)| host);
    build(host, path)
}

/// Recognize the scp-like shorthand `host:path` (no `://`), the form
/// `github.com:owner/repo.git` uses once [`strip_scheme_less_userinfo`] has
/// already removed any `user@` prefix.
fn parse_scp_like(input: &str) -> Option<(&str, &str)> {
    if input.contains("://") {
        return None;
    }
    let (host, path) = input.split_once(':')?;
    if host.is_empty() || host.contains('/') || path.is_empty() {
        return None;
    }
    Some((host, path))
}

/// Build the final `<host>/<path>` value: lowercase the host, strip
/// surrounding slashes and a trailing `.git` suffix from the path.
fn build(host: &str, path: &str) -> Result<NormalizedRemote, RemoteError> {
    if host.is_empty() {
        return Err(RemoteError::MissingHost);
    }
    let trimmed_path = path.trim_matches('/');
    let without_git_suffix = trimmed_path.strip_suffix(".git").unwrap_or(trimmed_path);
    let clean_path = without_git_suffix.trim_matches('/');
    if clean_path.is_empty() {
        return Err(RemoteError::MissingPath);
    }
    Ok(NormalizedRemote(format!(
        "{}/{clean_path}",
        host.to_lowercase()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scp_like_ssh_url_and_https_url_normalize_identically() {
        let expected = "github.com/tftio/kb";
        assert_eq!(
            expected,
            normalize_remote("git@github.com:tftio/kb.git")
                .expect("scp-like")
                .as_str()
        );
        assert_eq!(
            expected,
            normalize_remote("ssh://git@github.com/tftio/kb")
                .expect("ssh url")
                .as_str()
        );
        assert_eq!(
            expected,
            normalize_remote("https://github.com/tftio/kb")
                .expect("https url")
                .as_str()
        );
    }

    #[test]
    fn a_credential_never_appears_in_the_normalized_output() {
        let normalized = normalize_remote("https://user:token@host/x")
            .expect("credentialed url")
            .into_string();
        assert_eq!("host/x", normalized);
        assert!(!normalized.contains("token"));
    }

    #[test]
    fn a_credential_never_appears_in_an_error_message() {
        let err = normalize_remote("https://user:token@host").expect_err("no path after host");
        assert_eq!(RemoteError::MissingPath, err);
        assert!(!err.to_string().contains("token"));
    }

    #[test]
    fn http_and_git_schemes_normalize_like_https() {
        assert_eq!(
            "example.com/a/b",
            normalize_remote("http://example.com/a/b")
                .expect("http")
                .as_str()
        );
        assert_eq!(
            "example.com/a/b",
            normalize_remote("git://example.com/a/b.git")
                .expect("git")
                .as_str()
        );
    }

    #[test]
    fn ssh_url_with_a_port_drops_the_port() {
        assert_eq!(
            "example.com/a/b",
            normalize_remote("ssh://git@example.com:2222/a/b")
                .expect("ssh with port")
                .as_str()
        );
    }

    #[test]
    fn host_is_lowercased_and_path_case_is_preserved() {
        assert_eq!(
            "github.com/TFTio/KB",
            normalize_remote("https://GitHub.com/TFTio/KB")
                .expect("mixed case")
                .as_str()
        );
    }

    #[test]
    fn file_url_normalizes_under_the_local_pseudo_host() {
        assert_eq!(
            "local/home/user/kb",
            normalize_remote("file:///home/user/kb")
                .expect("file url")
                .as_str()
        );
    }

    #[test]
    fn a_bare_path_normalizes_the_same_as_the_matching_file_url() {
        assert_eq!(
            "local/home/user/kb",
            normalize_remote("/home/user/kb")
                .expect("bare path")
                .as_str()
        );
    }

    #[test]
    fn a_scheme_less_host_slash_path_string_is_read_as_already_normalized() {
        // No leading `/`, `./`, or `../` marker: read directly as
        // `<host>/<path>`, exactly as the registry stores it.
        assert_eq!(
            "repos/kb",
            normalize_remote("repos/kb")
                .expect("host/path text")
                .as_str()
        );
    }

    #[test]
    fn an_explicitly_relative_path_normalizes_under_the_local_pseudo_host() {
        assert_eq!(
            "local/repos/kb",
            normalize_remote("./repos/kb")
                .expect("relative path")
                .as_str()
        );
    }

    #[test]
    fn an_empty_string_is_rejected() {
        assert_eq!(RemoteError::Empty, normalize_remote("").unwrap_err());
        assert_eq!(RemoteError::Empty, normalize_remote("   ").unwrap_err());
    }

    #[test]
    fn a_url_with_a_host_but_no_path_is_rejected() {
        assert_eq!(
            RemoteError::MissingPath,
            normalize_remote("https://example.com").unwrap_err()
        );
    }

    #[test]
    fn repeated_normalization_is_idempotent() {
        let once = normalize_remote("git@github.com:tftio/kb.git")
            .expect("scp-like")
            .into_string();
        let twice = normalize_remote(&once)
            .expect("already normalized")
            .into_string();
        assert_eq!(once, twice);
    }

    #[test]
    fn display_renders_the_normalized_text() {
        let normalized = normalize_remote("https://github.com/tftio/kb").expect("https");
        assert_eq!("github.com/tftio/kb", normalized.to_string());
    }

    #[test]
    fn scp_like_detection_ignores_url_like_input() {
        // A `scheme://` string must never be mistaken for scp-like shorthand.
        assert_eq!(
            "github.com/tftio/kb",
            normalize_remote("ssh://git@github.com/tftio/kb.git")
                .expect("ssh url")
                .as_str()
        );
    }

    #[test]
    fn a_password_containing_at_signs_does_not_leak_a_host_fragment() {
        // The authority has two `@`s (one inside the password); only the
        // text after the LAST one is the host.
        assert_eq!(
            "host/x",
            normalize_remote("https://u:p@ss@host/x")
                .expect("credentialed url with an @ in the password")
                .as_str()
        );
    }

    #[test]
    fn an_at_sign_in_the_path_is_never_mistaken_for_userinfo() {
        assert_eq!(
            "host/owner/repo@v1",
            normalize_remote("https://host/owner/repo@v1")
                .expect("path containing an @")
                .as_str()
        );
    }

    #[test]
    fn scheme_less_scp_like_input_with_userinfo_strips_only_the_authority() {
        assert_eq!(
            "github.com/o/r",
            normalize_remote("u:tok@github.com:o/r")
                .expect("scheme-less scp-like with userinfo")
                .as_str()
        );
    }

    #[test]
    fn scheme_less_host_path_input_with_userinfo_strips_only_the_authority() {
        assert_eq!(
            "host/path",
            normalize_remote("u:tok@host/path")
                .expect("scheme-less host/path with userinfo")
                .as_str()
        );
    }

    #[test]
    fn no_credential_text_survives_in_any_ok_value_or_error_display() {
        // Every case here embeds a distinctive, searchable username and
        // secret. Whether the input normalizes or is rejected, neither
        // fragment may appear in the result.
        let cases: &[(&str, &str, &str)] = &[
            ("https://alice:s3cr3t@host/x", "alice", "s3cr3t"),
            ("https://alice:s3cr3t@host", "alice", "s3cr3t"),
            ("https://alice:p@ss@host/x", "alice", "p@ss"),
            ("usr:tok3n@github.com:o/r", "usr", "tok3n"),
            ("usr:tok3n@host/path", "usr", "tok3n"),
        ];

        for (input, username, secret) in cases {
            let text = match normalize_remote(input) {
                Ok(normalized) => normalized.into_string(),
                Err(err) => err.to_string(),
            };
            assert!(
                !text.contains(secret),
                "secret {secret:?} leaked from {input:?} into {text:?}"
            );
            assert!(
                !text.contains(username),
                "username {username:?} leaked from {input:?} into {text:?}"
            );
        }
    }

    #[test]
    fn normalization_of_userinfo_bearing_input_is_idempotent() {
        for input in [
            "https://user:token@host/x",
            "https://u:p@ss@host/x",
            "u:tok@github.com:o/r",
            "u:tok@host/path",
            "git@github.com:tftio/kb.git",
        ] {
            let once = normalize_remote(input).expect("normalizable").into_string();
            let twice = normalize_remote(&once)
                .expect("already normalized")
                .into_string();
            assert_eq!(once, twice, "not idempotent for {input:?}");
        }
    }
}
