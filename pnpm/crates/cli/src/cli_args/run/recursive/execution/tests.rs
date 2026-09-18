use super::{ProcessTracker, RunProjectProcess, ScriptBudget, start_script};
use std::sync::atomic::{AtomicBool, Ordering};

/// The run settings [`start_script`] reads. The budget holds one permit,
/// which is every permit a single-script assertion needs.
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

/// The permit a script wins after the run was cancelled is not a licence
/// to start it: `run_stages` spawns without consulting the tracker again.
#[test]
fn a_script_that_waited_out_the_runs_cancellation_takes_no_permit() {
    let script_budget = ScriptBudget::new(1);
    let process_tracker = ProcessTracker::foreground();
    process_tracker.cancel();
    let on_started = || {};

    let permit = start_script(&queued_script(&script_budget, &process_tracker, &on_started));

    assert!(permit.is_none());
}

#[test]
fn the_budget_holds_a_script_back_until_a_permit_frees() {
    let script_budget = ScriptBudget::new(1);
    let held = script_budget.acquire();
    let took_a_permit = AtomicBool::new(false);

    std::thread::scope(|scope| {
        scope.spawn(|| {
            let _permit = script_budget.acquire();
            took_a_permit.store(true, Ordering::SeqCst);
        });
        // Nothing can hold the budget's only permit while this one does,
        // however far the spawned script has got.
        assert!(!took_a_permit.load(Ordering::SeqCst));
        drop(held);
    });

    assert!(took_a_permit.load(Ordering::SeqCst));
}
