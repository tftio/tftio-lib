//! Public API integration tests for `tftio-lib`.

use tftio_lib::{AgentModeContext, DoctorCheck, RepoInfo};

#[test]
fn cli_common_surface_is_available_at_the_crate_root() {
    let repository = RepoInfo::new("example", "example");
    let check = DoctorCheck::pass("configuration");
    let agent = AgentModeContext::from_tokens(Some("token".into()), Some("token".into()));

    assert_eq!(repository.owner, "example");
    assert_eq!(repository.name, "example");
    assert!(check.passed);
    assert!(agent.active);
}
