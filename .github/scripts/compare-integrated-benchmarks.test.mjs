import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { test } from 'node:test'
import { compare, compareConfirmed, compareSyscalls, parseSyscallSummary, renderComparison, scenarios } from './compare-integrated-benchmarks.mjs'

function command (name, times) {
  return { command: name, times, exit_codes: times.map(() => 0) }
}

function report (head, base) {
  return { results: [command('pacquet@HEAD', head), command('pacquet@main', base)] }
}

const stable = Array(10).fill(1)

test('a slower machine affects both revisions without causing an alert', () => {
  assert.equal(compare(report(stable.map(x => x * 3.1), stable.map(x => x * 3)), 'pacquet').status, 'Within tolerance')
})

test('a consistent regression fails, an improvement passes', () => {
  assert.equal(compare(report(Array(9).fill(1.3), Array(9).fill(1)), 'pacquet').status, 'Regression')
  assert.equal(compare(report(stable, stable.map(x => x * 1.3)), 'pacquet').status, 'Within tolerance')
})

test('multi-second scenarios catch consistent slowdowns below 20%', () => {
  const result = compare(report(Array(10).fill(3.18), Array(10).fill(3)), 'pacquet')
  assert.equal(result.status, 'Regression')
  assert.equal(result.tolerance, 3 * 0.05)
})

test('short scenarios tolerate up to 2 ms but still catch larger slowdowns', () => {
  const base = Array(10).fill(0.008)
  assert.equal(compare(report(Array(10).fill(0.009), base), 'pacquet').status, 'Within tolerance')
  assert.equal(compare(report(Array(10).fill(0.010), base), 'pacquet').status, 'Within tolerance')
  const result = compare(report(Array(10).fill(0.011), base), 'pacquet')
  assert.equal(result.status, 'Regression')
  assert.equal(result.tolerance, 0.002)
})

test('exactly 5% is tolerated when the relative threshold is larger', () => {
  assert.equal(compare(report(Array(10).fill(1.05), stable), 'pacquet').status, 'Within tolerance')
})

test('a noisy slowdown is explicitly inconclusive', () => {
  assert.equal(compare(report([1, 1, 1.3, 1.3, 1.3, 1.3, 1.4, 1.4, 2, 3], stable), 'pacquet').status, 'Inconclusive (overlapping samples)')
})

test('isolated outliers neither create nor conceal a regression', () => {
  assert.equal(compare(report([...stable.slice(1), 10], stable), 'pacquet').status, 'Within tolerance')
  assert.equal(compare(report([0.5, ...Array(9).fill(1.3)], [...stable.slice(1), 10]), 'pacquet').status, 'Regression')
})

test('missing, duplicate, failed, undersampled and invalid data fail closed', () => {
  const valid = report(stable, stable)
  const invalid = [
    { results: [] },
    { results: [...valid.results, valid.results[0]] },
    report(stable.slice(0, 8), stable),
    report([...stable.slice(1), NaN], stable),
    report([...stable.slice(1), 0], stable),
    { results: [{ ...valid.results[0], exit_codes: [0] }, valid.results[1]] },
    { results: [{ ...valid.results[0], exit_codes: [1, ...Array(9).fill(0)] }, valid.results[1]] },
  ]
  for (const input of invalid) assert.throws(() => compare(input, 'pacquet'))
})

test('all scenarios and both engines are checked, and missing reports fail', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'benchmark-comparison-'))
  try {
    for (const scenario of scenarios) {
      const input = report(stable, stable)
      if (!scenario.includes('PEER_HEAVY') && !scenario.includes('LINKED_WORKSPACE') && !scenario.includes('INJECTED_WORKSPACE') && !scenario.includes('STORE_ONLY') && !scenario.startsWith('CARGO_')) {
        input.results.push(command('pnpr@HEAD', Array(10).fill(1.4)), command('pnpr@main', stable))
      }
      input.confirmation = { results: input.results }
      input.syscalls = Object.fromEntries(input.results.map(result => [result.command, [{ statx: 1000 }, { statx: 1000 }]]))
      await writeFile(join(directory, `BENCHMARK_REPORT_${scenario}.json`), JSON.stringify(input))
    }
    const result = await renderComparison(directory)
    assert.equal(result.failed, true)
    assert.equal(result.markdown.match(/\| Regression \|/g).length, 10)
    assert.equal(result.markdown.match(/\| Within tolerance \|/g).length, 15 + 25)
    const cli = new URL('./compare-integrated-benchmarks.mjs', import.meta.url)
    const run = spawnSync(process.execPath, [fileURLToPath(cli), directory], { encoding: 'utf8' })
    assert.equal(run.status, 1)
    assert.match(run.stdout, /Regression/)
    assert.equal(run.stderr, '')
    for (const scenario of scenarios) {
      const input = { results: ['pacquet', 'pnpr'].flatMap(engine => [command(`${engine}@HEAD`, stable), command(`${engine}@main`, stable)]) }
      input.confirmation = { results: input.results }
      input.syscalls = Object.fromEntries(input.results.map(result => [result.command, [{ statx: 1000 }, { statx: 1000 }]]))
      await writeFile(join(directory, `BENCHMARK_REPORT_${scenario}.json`), JSON.stringify(input))
    }
    assert.equal(spawnSync(process.execPath, [fileURLToPath(cli), directory]).status, 0)
    await rm(join(directory, `BENCHMARK_REPORT_${scenarios[0]}.json`))
    await assert.rejects(renderComparison(directory), /ENOENT/)
  } finally {
    await rm(directory, { recursive: true, force: true })
  }
})

test('reports from a harness that predates syscall counting render without the syscall rows', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'benchmark-comparison-'))
  try {
    for (const scenario of scenarios) {
      const input = { results: ['pacquet', 'pnpr'].flatMap(engine => [command(`${engine}@HEAD`, stable), command(`${engine}@main`, stable)]) }
      input.confirmation = { results: input.results }
      if (scenario !== 'ISOLATED_FRESH_RESTORE_HOT_CACHE_HOT_STORE') {
        input.syscalls = Object.fromEntries(input.results.map(result => [result.command, [{ statx: 1000 }, { statx: 1000 }]]))
      }
      await writeFile(join(directory, `BENCHMARK_REPORT_${scenario}.json`), JSON.stringify(input))
    }
    const result = await renderComparison(directory)
    assert.equal(result.failed, false)
    assert.equal(result.markdown.match(/\| Not recorded by this run's harness \|/g).length, 2)
    assert.match(result.markdown, /\n\| isolated-fresh-restore-hot-cache-hot-store \| pacquet \| — \| — \| — \| — \| — \| — \| Not recorded by this run's harness \|\n/)
    assert.match(result.markdown, /\n\| isolated-fresh-restore-hot-cache-hot-store \| pnpr \| — \| — \| — \| — \| — \| — \| Not recorded by this run's harness \|\n/)
  } finally {
    await rm(directory, { recursive: true, force: true })
  }
})

test('comparison and Bencher uploads cover every matrix scenario', async () => {
  const workflow = await readFile(new URL('../workflows/pacquet-integrated-benchmark.yml', import.meta.url), 'utf8')
  const tags = [...workflow.matchAll(/'([A-Z_]+):[a-z][^']+'/g)].map(match => match[1])
  const matrix = workflow.match(/        scenario:\n((?:          - [^\n]+\n)+)/)[1]
  const matrixTags = [...matrix.matchAll(/- ([a-z][\w.-]+)/g)].map(match => match[1]
    .replace('-linker.', '_').replaceAll(/[.-]/g, '_').toUpperCase())
  assert.deepEqual([...new Set(tags)].sort(), [...matrixTags].sort())
  assert.deepEqual([...scenarios].sort(), [...matrixTags].sort())
})

test('hyperfine command names identify targets when commands are shell scripts', () => {
  const input = report(stable, stable)
  input.results = input.results.map(result => ({ ...result, command_name: result.command, command: '/tmp/run.sh' }))
  assert.equal(compare(input, 'pacquet').status, 'Within tolerance')
})

test('regressions must reproduce in the confirmation run', () => {
  const input = report(Array(10).fill(1.3), stable)
  assert.throws(() => compareConfirmed(input, 'pacquet'), /Missing confirmation/)
  input.confirmation = report(stable, stable)
  assert.equal(compareConfirmed(input, 'pacquet').status, 'Inconclusive (not reproduced)')
  input.confirmation = report(Array(10).fill(1.3), stable)
  assert.equal(compareConfirmed(input, 'pacquet').status, 'Regression')
  input.confirmation = report([], stable)
  assert.throws(() => compareConfirmed(input, 'pacquet'), /samples/)
})

test('strace summaries parse every row and must add up to the total', () => {
  const summary = `% time     seconds  usecs/call     calls    errors syscall
------ ----------- ----------- --------- --------- ------------------
 66,50    0,005085        1271         4         2 wait4
  5,98    0,000457           3       135        41 statx
  3,30    0,000252           8        30           read
------ ----------- ----------- --------- --------- ------------------
100,00    0,007647          10       169        43 total
`
  assert.deepEqual(parseSyscallSummary(summary), { wait4: 4, statx: 135, read: 30 })
  assert.throws(() => parseSyscallSummary(summary.replace('169', '170')), /add up to 169 calls, its total row says 170/)
  assert.throws(() => parseSyscallSummary(summary.replace(/^100.*\n/m, '')), /add up to 169 calls, its total row says undefined/)
  assert.throws(() => parseSyscallSummary(summary + 'System call usage summary for 32 bit mode:\n'), /Unexpected strace summary line: System call usage summary for 32 bit mode:/)
  assert.throws(() => parseSyscallSummary(''), /add up to 0 calls, its total row says undefined/)
})

test('mkdir calls that fail with EEXIST are not counted', () => {
  const summary = `% time     seconds  usecs/call     calls    errors syscall
------ ----------- ----------- --------- --------- ------------------
 60,00    0,000600           5     11049      2349 mkdir
 40,00    0,000400           5       120           mkdirat
------ ----------- ----------- --------- --------- ------------------
100,00    0,001000           5     11169      2349 total
`
  assert.deepEqual(parseSyscallSummary(summary), { 'mkdir (succeeded)': 8700, 'mkdirat (succeeded)': 120 })
  assert.throws(() => parseSyscallSummary(summary.replace('2349 mkdir', '11050 mkdir')), { message: 'Unexpected strace summary line:  60,00    0,000600           5     11049      11050 mkdir' })
  const input = { syscalls: {
    'pacquet@HEAD': [{ 'mkdir (succeeded)': 8700 }, { 'mkdir (succeeded)': 8700 }],
    'pacquet@main': [{ 'mkdir (succeeded)': 8700 }, { 'mkdir (succeeded)': 8700 }],
  } }
  assert.deepEqual(compareSyscalls(input, 'pacquet'), [
    { syscall: 'mkdir (succeeded)', head: 8700, base: 8700, change: 0, spread: 0, tolerance: 174, status: 'Within tolerance' },
  ])
})

test('a doubled statx count is a syscall regression', () => {
  const input = { syscalls: {
    'pacquet@HEAD': [{ statx: 121177, read: 900, futex: 9000 }, { statx: 121180, read: 900, futex: 1 }],
    'pacquet@main': [{ statx: 53886, read: 900, futex: 5 }, { statx: 53889, read: 901, futex: 7000 }],
  } }
  assert.deepEqual(compareSyscalls(input, 'pacquet'), [
    { syscall: 'statx', head: 121177, base: 53889, change: 67288, spread: 3, tolerance: 1077, status: 'Regression' },
  ])
})

test('syscall changes within 2% or 50 calls pass, and an improvement passes', () => {
  const counts = (head, base) => ({ syscalls: {
    'pacquet@HEAD': [{ openat: head }, { openat: head }],
    'pacquet@main': [{ openat: base }, { openat: base }],
  } })
  assert.equal(compareSyscalls(counts(1050, 1000), 'pacquet')[0].status, 'Within tolerance')
  assert.equal(compareSyscalls(counts(1051, 1000), 'pacquet')[0].status, 'Regression')
  assert.equal(compareSyscalls(counts(10200, 10000), 'pacquet')[0].status, 'Within tolerance')
  assert.equal(compareSyscalls(counts(10201, 10000), 'pacquet')[0].status, 'Regression')
  assert.equal(compareSyscalls(counts(10201, 10001), 'pacquet')[0].status, 'Within tolerance')
  assert.equal(compareSyscalls(counts(10202, 10001), 'pacquet')[0].status, 'Regression')
  assert.equal(compareSyscalls(counts(500, 1000), 'pacquet')[0].status, 'Within tolerance')
  assert.deepEqual(compareSyscalls(counts(51, 0), 'pacquet'), [
    { syscall: 'openat', head: 51, base: 0, change: 51, spread: 0, tolerance: 50, status: 'Regression' },
  ])
})

test('identical runs that disagree beyond the tolerance are unstable, not gated', () => {
  const input = { syscalls: {
    'pacquet@HEAD': [{ getdents64: 1160 }, { getdents64: 1260 }],
    'pacquet@main': [{ getdents64: 1000 }, { getdents64: 1100 }],
  } }
  assert.deepEqual(compareSyscalls(input, 'pacquet'), [
    { syscall: 'getdents64', head: 1160, base: 1100, change: 60, spread: 100, tolerance: 50, status: 'Unstable (identical runs differ by 100, not gated)' },
  ])
})

test('a regression beyond both the tolerance and the run spread fails even when runs disagree', () => {
  const counts = (head, base) => ({ syscalls: {
    'pacquet@HEAD': [{ getdents64: head[0] }, { getdents64: head[1] }],
    'pacquet@main': [{ getdents64: base[0] }, { getdents64: base[1] }],
  } })
  assert.deepEqual(compareSyscalls(counts([3000, 3100], [1000, 1000]), 'pacquet'), [
    { syscall: 'getdents64', head: 3000, base: 1000, change: 2000, spread: 100, tolerance: 50, status: 'Regression' },
  ])
  assert.equal(compareSyscalls(counts([1250, 1300], [1000, 1100]), 'pacquet')[0].status, 'Unstable (identical runs differ by 100, not gated)')
  assert.equal(compareSyscalls(counts([1251, 1300], [1000, 1100]), 'pacquet')[0].status, 'Regression')
})

test('a run spread within the tolerance does not widen the gate', () => {
  const counts = (head, base) => ({ syscalls: {
    'pacquet@HEAD': [{ getdents64: head[0] }, { getdents64: head[1] }],
    'pacquet@main': [{ getdents64: base[0] }, { getdents64: base[1] }],
  } })
  assert.deepEqual(compareSyscalls(counts([1091, 1091], [1000, 1040]), 'pacquet'), [
    { syscall: 'getdents64', head: 1091, base: 1040, change: 51, spread: 40, tolerance: 50, status: 'Regression' },
  ])
  assert.equal(compareSyscalls(counts([1090, 1090], [1000, 1040]), 'pacquet')[0].status, 'Within tolerance')
})

test('missing, partial and invalid syscall counts fail closed', () => {
  const run = { statx: 1 }
  for (const [syscalls, message] of [
    [undefined, 'Expected 2 syscall count runs for pacquet@HEAD'],
    [{ 'pacquet@HEAD': [run, run] }, 'Expected 2 syscall count runs for pacquet@main'],
    [{ 'pacquet@HEAD': [run], 'pacquet@main': [run, run] }, 'Expected 2 syscall count runs for pacquet@HEAD'],
    [{ 'pacquet@HEAD': [run, run], 'pacquet@main': [run, { statx: -1 }] }, 'Invalid syscall counts for pacquet@main'],
    [{ 'pacquet@HEAD': [run, run], 'pacquet@main': [run, null] }, 'Invalid syscall counts for pacquet@main'],
  ]) {
    assert.throws(() => compareSyscalls({ syscalls }, 'pacquet'), { message })
  }
  assert.throws(() => compareSyscalls({ syscalls: { 'pacquet@HEAD': [{ futex: 1 }, { futex: 1 }], 'pacquet@main': [{ futex: 1 }, { futex: 1 }] } }, 'pacquet'), { message: 'No file syscalls were traced for pacquet' })
})

test('the syscall table shows regressions next to the timing table and fails the check', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'benchmark-comparison-'))
  try {
    for (const scenario of scenarios) {
      const input = { results: ['pacquet', 'pnpr'].flatMap(engine => [command(`${engine}@HEAD`, stable), command(`${engine}@main`, stable)]) }
      input.confirmation = { results: input.results }
      input.syscalls = Object.fromEntries(input.results.map(result => [result.command, [{ statx: 1000, openat: 400 }, { statx: 1000, openat: 400 }]]))
      if (scenario === scenarios[0]) input.syscalls['pacquet@HEAD'] = [{ statx: 2000, openat: 400 }, { statx: 2001, openat: 400 }]
      await writeFile(join(directory, `BENCHMARK_REPORT_${scenario}.json`), JSON.stringify(input))
    }
    const result = await renderComparison(directory)
    assert.equal(result.failed, true)
    assert.equal(result.markdown.match(/\| Regression \|/g).length, 1)
    assert.ok(result.markdown.includes(`| ${scenarios[0].toLowerCase().replaceAll('_', '-')} | pacquet | statx | 1000 | 2000 | +100.0% | 50 | 1 | Regression |`))
    assert.ok(result.markdown.indexOf('### Same-run regression check') < result.markdown.indexOf('### Same-run syscall check'))
  } finally {
    await rm(directory, { recursive: true, force: true })
  }
})
