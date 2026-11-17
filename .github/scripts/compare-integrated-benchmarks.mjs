import { readFile } from 'node:fs/promises'
import { join, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'

export const scenarios = [
  'ISOLATED_FRESH_RESTORE_COLD_CACHE_COLD_STORE',
  'ISOLATED_FRESH_RESTORE_HOT_CACHE_HOT_STORE',
  'ISOLATED_REPEAT_INSTALL_HOT_CACHE_HOT_STORE',
  'ISOLATED_REPEAT_INSTALL_COLD_CACHE_HOT_STORE',
  'ISOLATED_FRESH_INSTALL_COLD_CACHE_COLD_STORE',
  'ISOLATED_FRESH_INSTALL_HOT_CACHE_HOT_STORE',
  'ISOLATED_FRESH_INSTALL_COLD_CACHE_HOT_STORE',
  'ISOLATED_FRESH_RESOLVE_HOT_CACHE_OFFLINE',
  'ISOLATED_PEER_HEAVY_RESOLVE_HOT_CACHE_OFFLINE',
  'ISOLATED_LINKED_WORKSPACE_RESOLVE_HOT_CACHE_OFFLINE',
  'ISOLATED_INJECTED_WORKSPACE_REPEAT_INSTALL_HOT_CACHE_HOT_STORE',
  'ISOLATED_FRESH_RESTORE_COLD_CACHE_COLD_STORE_COLD_PNPR',
  'GVS_FRESH_RESTORE_HOT_CACHE_HOT_STORE',
  'STORE_ONLY_UNLOCKED_DIRECT_DEPS_COLD_CACHE_HOT_STORE',
  'CARGO_REPEAT_INSTALL_HOT_CACHE_HOT_STORE',
]

// Calls that create, inspect, open or materialize files. An install's file
// work changes these counts exactly, while wall time hides it under network
// and scheduler noise (pnpm/pnpm#17500 doubled statx at +5% CPU time).
export const gatedSyscalls = new Set([
  'statx', 'newfstatat', 'stat', 'lstat', 'fstat',
  'open', 'openat', 'openat2',
  'link', 'linkat', 'symlink', 'symlinkat', 'readlink', 'readlinkat',
  'copy_file_range', 'ioctl', 'getdents64',
  'mkdir (succeeded)', 'mkdirat (succeeded)', 'rename', 'renameat', 'renameat2', 'unlink', 'unlinkat',
  'chmod', 'fchmod', 'fchmodat', 'fchmodat2',
  'access', 'faccessat', 'faccessat2',
])
// Concurrent installs race to create the same directories, so how many mkdir
// calls fail with EEXIST varies between identical runs. Only the calls that
// created a directory are counted for these.
const successCountedSyscalls = new Set(['mkdir', 'mkdirat'])
export const syscallCountRounds = 2

export function compare (report, engine) {
  const head = samples(report, `${engine}@HEAD`)
  const base = samples(report, `${engine}@main`)
  const headMedian = median(head)
  const baseMedian = median(base)
  const tolerance = Math.max(baseMedian * 0.05, 0.002)
  const ratio = headMedian / baseMedian
  // Trim at most 10% of each tail. This is an empirical noise guard, not a
  // confidence interval: hyperfine measures commands sequentially.
  const headLow = head[Math.floor(head.length / 10)]
  const baseHigh = base[base.length - 1 - Math.floor(base.length / 10)]
  const status = headMedian <= baseMedian + tolerance ? 'Within tolerance' : headLow > baseHigh + tolerance
    ? 'Regression' : 'Inconclusive (overlapping samples)'
  return { head: headMedian, base: baseMedian, ratio, tolerance, status }
}

export function compareConfirmed (report, engine) {
  const initial = compare(report, engine)
  if (initial.status !== 'Regression') return initial
  if (!report.confirmation) throw new Error(`Missing confirmation run for ${engine}`)
  const confirmation = compare(report.confirmation, engine)
  return {
    ...initial,
    status: confirmation.status === 'Regression' ? 'Regression' : 'Inconclusive (not reproduced)',
  }
}

// Parse the table `strace --summary-only` writes. Every row must parse and add
// up to the total row, so a format change fails instead of dropping calls.
export function parseSyscallSummary (text) {
  const counts = {}
  let total
  let sum = 0
  for (const line of text.split('\n')) {
    const fields = line.trim().split(/\s+/)
    if (fields[0] === '' || fields[0] === '%' || /^-+$/.test(fields[0])) continue
    const name = fields.at(-1)
    const calls = Number(fields[3])
    // strace leaves the errors column empty when no call failed.
    const errors = fields.length === 6 ? Number(fields[4]) : 0
    if ((fields.length !== 5 && fields.length !== 6) || !Number.isInteger(calls) || calls < 0 ||
      !Number.isInteger(errors) || errors < 0 || errors > calls) {
      throw new Error(`Unexpected strace summary line: ${line}`)
    }
    if (name === 'total') {
      total = calls
      continue
    }
    sum += calls
    if (successCountedSyscalls.has(name)) counts[`${name} (succeeded)`] = calls - errors
    else counts[name] = calls
  }
  if (total === undefined || total !== sum) {
    throw new Error(`strace summary rows add up to ${sum} calls, its total row says ${total}`)
  }
  return counts
}

// Compare HEAD's fewest calls against main's most, per file syscall. When
// the two traced runs of a revision disagree beyond the tolerance, only a
// gap wider than that disagreement too is gated; smaller ones are reported
// unstable.
export function compareSyscalls (report, engine) {
  const head = syscallRuns(report, `${engine}@HEAD`)
  const base = syscallRuns(report, `${engine}@main`)
  const names = [...new Set([...head, ...base].flatMap(run => Object.keys(run)))]
    .filter(name => gatedSyscalls.has(name))
    .sort()
  if (names.length === 0) throw new Error(`No file syscalls were traced for ${engine}`)
  return names.map(syscall => {
    const headCounts = head.map(run => run[syscall] ?? 0)
    const baseCounts = base.map(run => run[syscall] ?? 0)
    const headLow = Math.min(...headCounts)
    const baseHigh = Math.max(...baseCounts)
    const spread = Math.max(Math.max(...headCounts) - headLow, baseHigh - Math.min(...baseCounts))
    const tolerance = Math.max(Math.floor(baseHigh * 0.02), 50)
    const unstable = spread > tolerance
    // Runs that disagree widen the gate by their spread, so noise alone
    // cannot open it; a gap wider than both is still a regression.
    const regressionGap = unstable ? tolerance + spread : tolerance
    const status = headLow > baseHigh + regressionGap ? 'Regression'
      : unstable ? `Unstable (identical runs differ by ${spread}, not gated)`
        : 'Within tolerance'
    return { syscall, head: headLow, base: baseHigh, change: headLow - baseHigh, spread, tolerance, status }
  })
}

function syscallRuns (report, name) {
  const runs = report.syscalls?.[name]
  if (!Array.isArray(runs) || runs.length !== syscallCountRounds) {
    throw new Error(`Expected ${syscallCountRounds} syscall count runs for ${name}`)
  }
  for (const run of runs) {
    if (run === null || typeof run !== 'object' || Object.values(run).some(calls => !Number.isInteger(calls) || calls < 0)) {
      throw new Error(`Invalid syscall counts for ${name}`)
    }
  }
  return runs
}

function samples (report, name) {
  const matches = report.results.filter(result => (result.command_name ?? result.command) === name)
  if (matches.length !== 1) throw new Error(`Expected one result for ${name}`)
  const { times, exit_codes: exitCodes } = matches[0]
  if (!Array.isArray(times) || times.length < 9 || times.some(time => !Number.isFinite(time) || time <= 0)) {
    throw new Error(`Expected at least 9 positive finite samples for ${name}`)
  }
  if (!Array.isArray(exitCodes) || exitCodes.length !== times.length || exitCodes.some(code => code !== 0)) {
    throw new Error(`Missing or unsuccessful exit codes for ${name}`)
  }
  return [...times].sort((a, b) => a - b)
}

function median (values) {
  const middle = Math.floor(values.length / 2)
  return values.length % 2 ? values[middle] : (values[middle - 1] + values[middle]) / 2
}

export async function renderComparison (directory) {
  const lines = [
    '### Same-run regression check',
    '',
    'HEAD and main are measured on the same runner. The tolerance is 5% of the main median or 2 ms, whichever is larger. A regression requires both the median slowdown and the gap between the fastest retained HEAD sample and slowest retained main sample to exceed this tolerance, after trimming up to 10% from each tail. Suspected regressions are rerun once with target order reversed and only fail if reproduced. This conservative noise guard is not a statistical confidence interval.',
    '',
    'Inconclusive rows do not fail the check. They remain visible for performance review. Bencher retains absolute minimum timings for historical tracking; historical alerts do not gate this PR. Sequential measurements can still be affected by runner drift.',
    '',
    '| Scenario | Engine | main median | HEAD median | Change | Tolerance | Result |',
    '| --- | --- | ---: | ---: | ---: | ---: | --- |',
  ]
  const syscallLines = [
    '',
    '### Same-run syscall check',
    '',
    `Each install runs ${syscallCountRounds} more times per revision, untimed, under \`strace --follow-forks --summary-only\`. A file syscall regresses when HEAD's fewest calls exceed main's most by more than 2% of main or 50 calls, whichever is larger. mkdir and mkdirat count only the calls that created a directory, since concurrent installs fail a varying number with EEXIST. A syscall whose identical runs differ by more than that tolerance is reported unstable and not gated, unless HEAD's fewest calls also exceed main's most by more than the tolerance plus that difference. Each scenario lists its largest change relative to its tolerance and every row that is not within tolerance.`,
    '',
    '| Scenario | Engine | Syscall | main calls (max) | HEAD calls (min) | Change | Tolerance | Identical-run spread | Result |',
    '| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | --- |',
  ]
  let failed = false
  for (const scenario of scenarios) {
    const report = JSON.parse(await readFile(join(directory, `BENCHMARK_REPORT_${scenario}.json`), 'utf8'))
    const engines = scenario.includes('PEER_HEAVY') || scenario.includes('LINKED_WORKSPACE') || scenario.includes('INJECTED_WORKSPACE') || scenario.includes('STORE_ONLY') || scenario.startsWith('CARGO_') ? ['pacquet'] : ['pacquet', 'pnpr']
    const label = scenario.toLowerCase().replaceAll('_', '-')
    for (const engine of engines) {
      const result = compareConfirmed(report, engine)
      failed ||= result.status === 'Regression'
      lines.push(`| ${label} | ${engine} | ${(result.base * 1000).toFixed(2)} ms | ${(result.head * 1000).toFixed(2)} ms | ${((result.ratio - 1) * 100).toFixed(1)}% | ${(result.tolerance * 1000).toFixed(2)} ms | ${result.status} |`)
    }
    for (const engine of engines) {
      // A report written without --count-syscalls carries none.
      if (report.syscalls === undefined) {
        syscallLines.push(`| ${label} | ${engine} | — | — | — | — | — | — | Not recorded by this run's harness |`)
        continue
      }
      const rows = compareSyscalls(report, engine)
      failed ||= rows.some(row => row.status === 'Regression')
      const largest = rows.reduce((left, right) => right.change / right.tolerance > left.change / left.tolerance ? right : left)
      for (const row of rows.filter(row => row === largest || row.status !== 'Within tolerance')) {
        const change = row.base === 0 ? 'new' : `${row.change >= 0 ? '+' : ''}${(row.change / row.base * 100).toFixed(1)}%`
        syscallLines.push(`| ${label} | ${engine} | ${row.syscall} | ${row.base} | ${row.head} | ${change} | ${row.tolerance} | ${row.spread} | ${row.status} |`)
      }
    }
  }
  return { markdown: [...lines, ...syscallLines].join('\n') + '\n', failed }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const { markdown, failed } = await renderComparison(process.argv[2])
  console.log(markdown)
  process.exitCode = failed ? 1 : 0
}
