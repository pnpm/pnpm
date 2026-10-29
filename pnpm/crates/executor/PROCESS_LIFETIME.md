# Process lifetime

This is the contract for what happens to the processes pnpm starts for
scripts and commands (`run`, `exec`, `dlx`, lifecycle scripts, and their
recursive forms) when they, or pnpm, end. Every row below names the tests that
pin it. Unless a path says otherwise, they are in
[`cli/tests/suite/process_lifetime/`](../cli/tests/suite/process_lifetime/).

## The rule

What a command leaves running outlives pnpm exactly when it would outlive the
command run without pnpm. Cargo, `turbo exec`, npm, `deno task`, just, and moon
follow the same rule.

pnpm differs from it on purpose in two places only. Each is listed below with
its reason.

1. When **pnpm itself is killed or crashes**, what it started ends with it,
   within the limits each platform sets (see below).
2. When a **bail cancels the tasks of a multi-task run**, pnpm stops those
   tasks.

## Single command

| pnpm ends because | What the command left running | Tests |
| --- | --- | --- |
| the command succeeded or failed | keeps running | `single_command::*`, for `exec`, `run`, `dlx`, a lifecycle script, and on Windows `exec` with pnpm launched by Node |
| the user pressed `Ctrl+C` | keeps running, unless the interrupt itself ended it, as it would without pnpm | `interrupt::ctrl_c_leaves_the_same_processes_running_as_without_pnpm` |
| on Windows, a second `Ctrl+C` while the command ignores them | pnpm ends the command itself and then exits on its own, so what the command left running keeps running | `interrupt::repeated_ctrl_c_ends_a_command_that_ignores_it_but_not_its_leftovers` |
| on Unix, a third `Ctrl+C` while the command ignores them | pnpm dies of the interrupt, which is difference 1 | `a_third_interrupt_ends_pnpm_even_when_the_script_ignores_them` in [`suite/interrupt.rs`](../cli/tests/suite/interrupt.rs) |
| the terminal was closed | the operating system ends what is attached to the terminal; everything else keeps running | `interrupt::a_closing_terminal_leaves_the_same_processes_running_as_without_pnpm` |
| pnpm was killed or crashed | **ends**, within the platform limits of difference 1 | `killed::*`, and `killing_the_process_group_of_pnpm_*` in [`suite/interrupt.rs`](../cli/tests/suite/interrupt.rs) |

A program a script opened through the shell in a console of its own, such as a
browser opened by the `open` package or an editor started with `start`, is not
attached to the terminal. It therefore keeps running in every row but the last
([pnpm/pnpm#17465](https://github.com/pnpm/pnpm/issues/17465),
[pnpm/pnpm#14356](https://github.com/pnpm/pnpm/issues/14356)). A program
started with `start /b` shares the terminal's console instead, so closing the
terminal ends it, as it would without pnpm.

Difference 1 exists so that a pnpm stopped by its parent, such as a CI runner
cancelling a job or Node killing the pnpm it launched, does not leave a dev
server holding its port. How far it reaches depends on the platform:

- **Windows:** the Job Object ends everything pnpm started, detached processes
  included. Only a process that asks for `CREATE_BREAKAWAY_FROM_JOB` is exempt
  ([pnpm/pnpm#16628](https://github.com/pnpm/pnpm/issues/16628)), which
  `breakaway::*` pins. A `Ctrl+C` that arrives while pnpm runs no command, for
  example between two lifecycle scripts, is not a kill: it ends pnpm and
  leaves what earlier scripts started running
  (`an_interrupt_while_no_command_runs_releases_the_process_tree_cleanup` in
  [`src/interrupt/windows/tests.rs`](./src/interrupt/windows/tests.rs)). Every
  child that joins a job is created suspended and resumed once it is in, so
  nothing it starts can escape the job in between.
- **Unix:** a script that runs in a process group of its own, which every
  script of a parallel run and every script without a controlling terminal
  does, is watched, and its group is killed when pnpm dies
  ([pnpm/pnpm#15555](https://github.com/pnpm/pnpm/issues/15555)). A script
  that shares the terminal's process group is not watched: a signal sent to
  that group, such as the one a closing terminal sends, reaches it directly,
  and a signal sent to pnpm alone does not. A process that leaves its group,
  such as a daemon calling `setsid`, keeps running in either case. That pnpm's
  own exit is not taken for its death is pinned by
  `a_process_the_script_left_behind_survives_the_exit_of_pnpm` in
  [`suite/interrupt.rs`](../cli/tests/suite/interrupt.rs).

## Multi-task runs

A recursive or parallel run follows the single-command table for every task
that ends on its own, including the leftovers of a task that succeeded or
failed (`multi_task::leftovers_of_a_finished_script_survive_a_recursive_run`).
pnpm 11 and Nx do the same, while `turbo run` kills the leftovers of a
finished task.

When a task fails under bail (the default), pnpm stops the tasks that are
still running (difference 2), as `turbo run`, moon, and pnpm 11 do. The task
whose failure caused the bail has already ended, so its leftovers keep running
(`multi_task::leftovers_of_the_script_a_bail_follows_survive_it`).

- **Unix:** each task's process group, or the task when it shares the
  terminal's group, gets `SIGTERM` once, and so does every descendant found
  through parent links that the group signal did not reach. Two seconds
  later, whatever is still provably the task gets `SIGKILL`: a group that
  still has a member, a task pnpm has not reaped yet, and that task's
  descendants as a fresh scan finds them. A pid recorded two seconds earlier
  may name another process by then, so nothing is killed by a stale pid.
  pnpm does not exit before those two seconds have passed or the tasks have
  ended, whichever comes first
  (`multi_task::recursive_run_bail_stops_running_scripts_gracefully_then_kills_them`).
- **Windows:** there is no signal that reaches one task without reaching
  pnpm's console, so each task's Job Object is terminated at once. The job
  holds everything the task started, including detached processes whose
  parent has exited, without depending on parent links, which a recycled pid
  could misdirect
  (`multi_task::recursive_run_bail_ends_a_detached_grandchild_of_a_running_script`).

On Unix a cancelled task's detached process is found only through its
parent. One whose parent still runs is stopped with the task. One whose
parent has exited, such as a daemon that forks twice, is not found and keeps
running. This is the one place where the two platforms differ.

`Ctrl+C` during a multi-task run reaches every task through the terminal.
pnpm waits for them, and further `Ctrl+C` escalate as described in
[`interrupt.rs`](./src/interrupt.rs).

## How the code keeps to it

- [`job_control.rs`](./src/job_control.rs) ties everything pnpm starts to a
  Windows Job Object with `KILL_ON_JOB_CLOSE` (difference 1), and gives each
  task a bail may cancel a nested job of its own (difference 2).
- [`exit.rs`](./src/exit.rs) is the only way pnpm exits on its own. It lets the
  cancelled tasks finish their grace, then releases the Job Object, so no exit
  ends what a command left running. `clippy.toml` bans `std::process::exit`
  everywhere else, and `main` calls `before_exit` before it returns.
- [`interrupt/windows.rs`](./src/interrupt/windows.rs) releases the Job Object
  when the console closes, because the operating system ends the console's
  own processes itself.
- [`process_tracker.rs`](./src/process_tracker.rs) implements the bail
  cancellation.

## Changing the contract

A request to end, or to keep, some process that this contract says otherwise
about is a change to the contract, not a bug fix. Make it here first. Change
the table, give the reason for any new difference from the command run
without pnpm, and update the tests the changed rows name. Do not change one of
those tests on its own: a test that fails after a change shows that the change
diverges from this contract.
