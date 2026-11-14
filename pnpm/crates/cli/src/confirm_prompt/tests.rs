use super::PromptAvailability;

#[test]
fn ci_disables_prompt_availability() {
    assert_eq!(PromptAvailability::detect(true), PromptAvailability::Unavailable);
    assert!(PromptAvailability::Available.is_available());
    assert!(!PromptAvailability::Unavailable.is_available());
}
