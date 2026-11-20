use super::{
    ProcessTracker,
    RunProjectProcess,
    ScriptBudget,
    start_script,
};
use std::sync::atomic::{
    AtomicBool,
    Ordering,
};

/// The run settings [`start_script`] reads.
fn queued_script<'a>(
    script_budget: &'a ScriptBudget,
    process_tracker: &'a ProcessTracker,
    on_started: &'a (dyn Fn() + Sync),
) -> RunProjectProcess<'a> {
    RunProjectProcess {
        bail: true,
        process_tracker: Some(process_tracker),
        script_budget,
        on_started,
    }
}

#[test]
fn a_script_takes_a_permit_while_the_run_is_live() {
    let script_budget = ScriptBudget::new(1);
    let process_tracker = ProcessTracker::foreground();
    let on_started = || {};

    let permit = start_script(&queued_script(&script_budget, &process_tracker, &on_started));

    assert!(permit.is_some());
}

/// The permit a script wins only after the run was cancelled is not a
/// licence to start it: `run_stages` reaches `run_script` without
/// consulting the tracker again.
///
/// This thread holds the budget's only permit, so the script's
/// `acquire` cannot return before the cancellation and the release
/// below, whichever order the two threads are scheduled in. The scope
/// also only joins once the freed permit reaches the waiting script.
#[test]
fn a_script_that_waits_out_the_runs_cancellation_takes_no_permit() {
    let script_budget = ScriptBudget::new(1);
    let process_tracker = ProcessTracker::foreground();
    let on_started = || {};
    let held = script_budget.acquire();
    // A script that never ran must fail the assertion, not pass it.
    let took_a_permit = AtomicBool::new(true);

    std::thread::scope(|scope| {
        scope.spawn(|| {
            let queued = queued_script(&script_budget, &process_tracker, &on_started);
            took_a_permit.store(start_script(&queued).is_some(), Ordering::SeqCst);
        });
        process_tracker.cancel();
        drop(held);
    });

    assert!(!took_a_permit.load(Ordering::SeqCst));
}
