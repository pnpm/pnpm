use super::SignalRegistry;

#[test]
fn a_registered_value_is_walked_until_its_guard_drops() {
    static REGISTRY: SignalRegistry<String> = SignalRegistry::new();
    let guard = REGISTRY.register("staged".to_owned());
    assert_eq!(visited(&REGISTRY), ["staged"]);
    drop(guard);
    assert!(visited(&REGISTRY).is_empty());
}

#[test]
fn every_live_registration_is_walked() {
    static REGISTRY: SignalRegistry<String> = SignalRegistry::new();
    let _first = REGISTRY.register("first".to_owned());
    let second = REGISTRY.register("second".to_owned());
    assert_eq!(visited(&REGISTRY), ["first", "second"]);
    drop(second);
    assert_eq!(visited(&REGISTRY), ["first"]);
}

/// Pins the reuse that bounds the registry's size: a slot a guard released
/// takes the next registration rather than growing the list.
#[test]
fn a_released_slot_is_reused() {
    static REGISTRY: SignalRegistry<String> = SignalRegistry::new();
    drop(REGISTRY.register("first".to_owned()));
    let _second = REGISTRY.register("second".to_owned());
    assert_eq!(visited(&REGISTRY), ["second"]);
}

/// Pins that releasing inside a walk neither frees the value under the walk
/// nor leaves the slot occupied afterwards, which is the race the walk
/// counter exists for.
#[test]
fn releasing_inside_a_walk_leaves_the_slot_free() {
    static REGISTRY: SignalRegistry<String> = SignalRegistry::new();
    let mut guard = Some(REGISTRY.register("staged".to_owned()));
    REGISTRY.for_each(|value| {
        assert_eq!(value, "staged");
        guard.take();
        // The value is still readable: the walk has not ended, so the
        // release left it alone.
        assert_eq!(value, "staged");
    });
    assert!(visited(&REGISTRY).is_empty());
    let _reused = REGISTRY.register("next".to_owned());
    assert_eq!(visited(&REGISTRY), ["next"]);
}

/// Pins that a walk is reentrant, which a signal handler needs: it runs one
/// inside whatever the interrupted thread was doing, possibly another walk.
#[test]
fn a_walk_inside_a_walk_sees_the_same_values() {
    static REGISTRY: SignalRegistry<String> = SignalRegistry::new();
    let _guard = REGISTRY.register("staged".to_owned());
    let mut inner = Vec::new();
    REGISTRY.for_each(|_| inner = visited(&REGISTRY));
    assert_eq!(inner, ["staged"]);
}

fn visited(registry: &SignalRegistry<String>) -> Vec<String> {
    let mut seen = Vec::new();
    registry.for_each(|value| seen.push(value.clone()));
    seen.sort();
    seen
}
