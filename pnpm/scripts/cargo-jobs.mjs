import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { pathToFileURL } from 'node:url'

const GIB = 1024 ** 3

// A rustc job compiling this workspace's heaviest crates holds ~1.6 GiB
// resident, and the linkers that overlap the tail of a build another ~0.5 GiB
// each. Budgeting 4 GiB a job keeps a full `--all-targets` build of pnpm and
// pnpr inside about half the machine, which is what the rest of the desktop
// and the pipelines parked on the `cargo` concurrency group need to survive
// it. Cargo and nextest both default to the core count instead, and a host
// with many cores per gigabyte fills its swap and gets killed by the OOM
// reaper before the build ends.
const GIB_PER_JOB = 4

/**
 * How many rustc jobs or test processes a machine can hold at once: one per
 * [`GIB_PER_JOB`] of memory, never more than it has cores to run.
 */
export function jobsForMachine (cpus, totalMemoryBytes) {
  return Math.max(1, Math.min(cpus, Math.floor(totalMemoryBytes / GIB / GIB_PER_JOB)))
}

/**
 * The `CARGO_BUILD_JOBS` and `NEXTEST_TEST_THREADS` a cargo run should carry.
 *
 * `CARGO_BUILD_JOBS` is the one knob: set it and the test threads follow it,
 * so `CARGO_BUILD_JOBS=4 just test` throttles the whole run. An explicit
 * `NEXTEST_TEST_THREADS` still wins for the test phase alone.
 */
export function parallelismEnv (env = process.env) {
  const jobs = env.CARGO_BUILD_JOBS ?? String(jobsForMachine(os.availableParallelism(), os.totalmem()))
  return {
    CARGO_BUILD_JOBS: jobs,
    NEXTEST_TEST_THREADS: env.NEXTEST_TEST_THREADS ?? jobs,
  }
}

if (process.argv[1] && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url) {
  process.stdout.write(`${parallelismEnv().CARGO_BUILD_JOBS}\n`)
}
