# Process lifetime

This is the contract for what happens to the processes pnpm starts for
scripts and commands (`run`, `exec`, `dlx`, lifecycle scripts, and their
recursive forms) when they, or pnpm, end. Change it here first, then in the
code and in [`process_lifetime.rs`](../cli/tests/suite/process_lifetime.rs),
which checks the single-command half against the same command run without
pnpm.

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

| pnpm ends because | What the command left running |
| --- | --- |
| the command succeeded | keeps running |
| the command failed | keeps running |
| the user pressed `Ctrl+C` and pnpm exited after the command | keeps running, unless the interrupt itself ended it, as it would without pnpm |
| the terminal was closed | the operating system ends what is attached to the terminal; everything else keeps running |
| pnpm was killed, crashed, or was forced down by repeated `Ctrl+C` | **ends**, within the platform limits of difference 1 |

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
  ([pnpm/pnpm#16628](https://github.com/pnpm/pnpm/issues/16628)).
- **Unix:** a script that runs in a process group of its own, which every
  script of a parallel run and every script without a controlling terminal
  does, is watched, and its group is killed when pnpm dies
  ([pnpm/pnpm#15555](https://github.com/pnpm/pnpm/issues/15555)). A script
  that shares the terminal's process group is not watched: a signal sent to
  that group, such as the one a closing terminal sends, reaches it directly,
  and a signal sent to pnpm alone does not. A process that leaves its group,
  such as a daemon calling `setsid`, keeps running in either case.

## Multi-task runs

A recursive or parallel run follows the single-command table for every task
that ends on its own, including the leftovers of a task that succeeded or
failed. pnpm 11 and Nx do the same, while `turbo run` kills the leftovers of a
finished task.

When a task fails under bail (the default), pnpm stops the tasks that are
still running (difference 2), as `turbo run`, moon, and pnpm 11 do:

- **Unix:** each task's process group, or the task and its descendants when it
  shares the terminal's group, gets `SIGTERM`. Whatever is still running two
  seconds later gets `SIGKILL`. pnpm does not exit before those two seconds
  have passed or the tasks have ended, whichever comes first.
- **Windows:** there is no signal that reaches one task without reaching
  pnpm's console, so each task's Job Object is terminated at once. The job
  holds everything the task started, including detached processes whose
  parent has exited, without depending on parent links, which a recycled pid
  could misdirect.

A detached process that a cancelled task started on Unix leaves the task's
process group, so it keeps running there. This is the one place where the two
platforms differ.

`Ctrl+C` during a multi-task run reaches every task through the terminal.
pnpm waits for them, and a second and third `Ctrl+C` escalate as described in
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
