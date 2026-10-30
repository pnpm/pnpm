import assert from 'node:assert/strict'
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { compareConfirmed } from './compare-integrated-benchmarks.mjs'
import { runBenchmark } from './run-integrated-benchmark.mjs'

async function writeSyscallCounts (directory, summary = `% time     seconds  usecs/call     calls    errors syscall
------ ----------- ----------- --------- --------- ------------------
  5,98    0,000457           3       135        41 statx
  3,30    0,000252           8        30           read
------ ----------- ----------- --------- --------- ------------------
100,00    0,000709           4       165        41 total
`) {
  for (const target of ['pacquet@HEAD', 'pacquet@main']) {
    await mkdir(join(directory, target), { recursive: true })
    for (const round of [0, 1]) await writeFile(join(directory, target, `SYSCALL_COUNTS.${round}.txt`), summary)
  }
}

function report (head) {
  return { results: ['pacquet@HEAD', 'pacquet@main'].map(command => ({
    command,
    times: Array(10).fill(command.endsWith('@HEAD') ? head : 1),
    exit_codes: Array(10).fill(0),
  })) }
}

for (const [name, initial, confirmation, expectedCalls, expectedStatus] of [
  ['stable results need no confirmation', 1, 1, 1, 'Within tolerance'],
  ['transient slowdowns are not reproduced', 1.3, 1, 2, 'Inconclusive (not reproduced)'],
  ['sustained regressions remain failures', 1.3, 1.3, 2, 'Regression'],
]) {
  test(name, async () => {
    const directory = await mkdtemp(join(tmpdir(), 'benchmark-confirmation-'))
    const calls = []
    const args = ['--scenario=example', '--runs=9', 'pacquet@HEAD', 'pacquet@main']
    try {
      const status = await runBenchmark(args, { directory, execute: async received => {
        calls.push(received)
        await writeSyscallCounts(directory)
        await writeFile(join(directory, 'BENCHMARK_REPORT.json'), JSON.stringify(report(calls.length === 1 ? initial : confirmation)))
        await writeFile(join(directory, 'BENCHMARK_REPORT.md'), `Run ${calls.length}`)
        return 0
      } })
      assert.equal(status, 0)
      assert.equal(calls.length, expectedCalls)
      assert.deepEqual(calls[0], [...args, '--count-syscalls'])
      if (expectedCalls === 2) {
        assert.deepEqual(calls[1], ['--scenario=example', '--runs=9', 'pacquet@main', 'pacquet@HEAD'])
        assert.match(await readFile(join(directory, 'BENCHMARK_REPORT.md'), 'utf8'), /Run 1[\s\S]*Confirmation[\s\S]*Run 2/)
      }
      const result = JSON.parse(await readFile(join(directory, 'BENCHMARK_REPORT.json'), 'utf8'))
      assert.equal(compareConfirmed(result, 'pacquet').status, expectedStatus)
      assert.deepEqual(result.syscalls, {
        'pacquet@HEAD': [{ statx: 135, read: 30 }, { statx: 135, read: 30 }],
        'pacquet@main': [{ statx: 135, read: 30 }, { statx: 135, read: 30 }],
      })
    } finally {
      await rm(directory, { recursive: true, force: true })
    }
  })
}

test('initial command failures are not retried or swallowed', async () => {
  let calls = 0
  assert.equal(await runBenchmark(['pacquet@HEAD', 'pacquet@main'], { execute: () => { calls++; return 7 } }), 7)
  assert.equal(calls, 1)
})

test('main-only runs do not require comparison reports or syscall counts', async () => {
  const calls = []
  assert.equal(await runBenchmark(['pacquet@HEAD', 'pnpr@HEAD'], { execute: received => { calls.push(received); return 0 } }), 0)
  assert.deepEqual(calls, [['pacquet@HEAD', 'pnpr@HEAD']])
})

test('missing or malformed syscall counts fail the run', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'benchmark-confirmation-'))
  try {
    const execute = async () => {
      await writeFile(join(directory, 'BENCHMARK_REPORT.json'), JSON.stringify(report(1)))
      await writeFile(join(directory, 'BENCHMARK_REPORT.md'), 'Initial measurement')
      return 0
    }
    await assert.rejects(runBenchmark(['pacquet@HEAD', 'pacquet@main'], { directory, execute }), {
      message: `Cannot read syscall counts from ${join(directory, 'pacquet@HEAD', 'SYSCALL_COUNTS.0.txt')}`,
    })
    await writeSyscallCounts(directory, 'garbage\n')
    await assert.rejects(runBenchmark(['pacquet@HEAD', 'pacquet@main'], { directory, execute }), error => {
      assert.equal(error.message, `Cannot read syscall counts from ${join(directory, 'pacquet@HEAD', 'SYSCALL_COUNTS.0.txt')}`)
      assert.equal(error.cause.message, 'Unexpected strace summary line: garbage')
      return true
    })
  } finally {
    await rm(directory, { recursive: true, force: true })
  }
})

test('confirmation command failures propagate and retain the initial report', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'benchmark-confirmation-'))
  let calls = 0
  try {
    const status = await runBenchmark(['pacquet@HEAD', 'pacquet@main'], { directory, execute: async () => {
      if (++calls === 2) return 7
      await writeSyscallCounts(directory)
      await writeFile(join(directory, 'BENCHMARK_REPORT.json'), JSON.stringify(report(1.3)))
      await writeFile(join(directory, 'BENCHMARK_REPORT.md'), 'Initial measurement')
      return 0
    } })
    assert.equal(status, 7)
    const result = JSON.parse(await readFile(join(directory, 'BENCHMARK_REPORT.json'), 'utf8'))
    assert.deepEqual(result, { ...report(1.3), syscalls: {
      'pacquet@HEAD': [{ statx: 135, read: 30 }, { statx: 135, read: 30 }],
      'pacquet@main': [{ statx: 135, read: 30 }, { statx: 135, read: 30 }],
    } })
    assert.match(await readFile(join(directory, 'BENCHMARK_REPORT.md'), 'utf8'), /Confirmation run failed/)
  } finally {
    await rm(directory, { recursive: true, force: true })
  }
})
