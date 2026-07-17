use crate::skill_runtime::vault_mutation::{
    VaultMutationAction, VaultMutationAuthorization, VaultMutationTool,
};

#[test]
fn vault_mutation_preview_authorization_matrix_is_captured_before_planning() {
    let cases = [
        ("@vault read notes/today.md", None, false),
        (
            "@vault write notes/today.md",
            Some(VaultMutationAction::Write),
            false,
        ),
        (
            "@vault write and overwrite notes/today.md",
            Some(VaultMutationAction::Write),
            true,
        ),
        (
            "@vault restore notes/today.md",
            Some(VaultMutationAction::Restore),
            false,
        ),
        (
            "@vault restore and overwrite notes/today.md",
            Some(VaultMutationAction::Restore),
            true,
        ),
        (
            "@vault move notes/today.md to Archive",
            Some(VaultMutationAction::Move),
            false,
        ),
        (
            "@vault move and overwrite notes/today.md to Archive",
            Some(VaultMutationAction::Move),
            true,
        ),
    ];

    for (request, expected_action, expected_overwrite) in cases {
        let authorization =
            VaultMutationAuthorization::capture(request).expect("parse authorization");
        assert_eq!(authorization.map(|value| value.action()), expected_action);
        assert_eq!(
            authorization.is_some_and(|value| value.overwrite_allowed()),
            expected_overwrite
        );
    }
}

#[test]
fn vault_mutation_preview_model_tool_cannot_elevate_user_authorization() {
    let authorization = VaultMutationAuthorization::capture("@vault write notes/today.md")
        .expect("parse authorization")
        .expect("write authorization");

    assert!(authorization.permits(VaultMutationTool::Write));
    assert!(!authorization.permits(VaultMutationTool::Restore));
    assert!(!authorization.permits(VaultMutationTool::Move));
    assert!(!authorization.overwrite_allowed());
}

#[test]
fn vault_mutation_preview_rejects_ambiguous_mutation_verbs() {
    let result = VaultMutationAuthorization::capture("@vault write then move notes/today.md");

    assert!(result.is_err());
}
