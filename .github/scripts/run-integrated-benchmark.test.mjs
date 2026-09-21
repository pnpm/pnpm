import assert from 'node:assert/strict'
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { compareConfirmed } from './compare-integrated-benchmarks.mjs'
import { runBenchmark } from './run-integrated-benchmark.mjs'

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
        await writeFile(join(directory, 'BENCHMARK_REPORT.json'), JSON.stringify(report(calls.length === 1 ? initial : confirmation)))
        await writeFile(join(directory, 'BENCHMARK_REPORT.md'), `Run ${calls.length}`)
        return 0
      } })
      assert.equal(status, 0)
      assert.equal(calls.length, expectedCalls)
      assert.deepEqual(calls[0], args)
      if (expectedCalls === 2) {
        assert.deepEqual(calls[1], ['--scenario=example', '--runs=9', 'pacquet@main', 'pacquet@HEAD'])
        assert.match(await readFile(join(directory, 'BENCHMARK_REPORT.md'), 'utf8'), /Run 1[\s\S]*Confirmation[\s\S]*Run 2/)
      }
      const result = JSON.parse(await readFile(join(directory, 'BENCHMARK_REPORT.json'), 'utf8'))
      assert.equal(compareConfirmed(result, 'pacquet').status, expectedStatus)
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

test('main-only runs do not require comparison reports', async () => {
  assert.equal(await runBenchmark(['pacquet@HEAD', 'pnpr@HEAD'], { execute: () => 0 }), 0)
})

test('confirmation command failures propagate and retain the initial report', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'benchmark-confirmation-'))
  let calls = 0
  try {
    const status = await runBenchmark(['pacquet@HEAD', 'pacquet@main'], { directory, execute: async () => {
      if (++calls === 2) return 7
      await writeFile(join(directory, 'BENCHMARK_REPORT.json'), JSON.stringify(report(1.3)))
      await writeFile(join(directory, 'BENCHMARK_REPORT.md'), 'Initial measurement')
      return 0
    } })
    assert.equal(status, 7)
    const result = JSON.parse(await readFile(join(directory, 'BENCHMARK_REPORT.json'), 'utf8'))
    assert.deepEqual(result, report(1.3))
    assert.match(await readFile(join(directory, 'BENCHMARK_REPORT.md'), 'utf8'), /Confirmation run failed/)
  } finally {
    await rm(directory, { recursive: true, force: true })
  }
})
