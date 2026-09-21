import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { test } from 'node:test'
import { compare, renderComparison, scenarios } from './compare-integrated-benchmarks.mjs'

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
      if (!scenario.includes('PEER_HEAVY') && !scenario.includes('LINKED_WORKSPACE')) {
        input.results.push(command('pnpr@HEAD', Array(10).fill(1.4)), command('pnpr@main', stable))
      }
      await writeFile(join(directory, `BENCHMARK_REPORT_${scenario}.json`), JSON.stringify(input))
    }
    const result = await renderComparison(directory)
    assert.equal(result.failed, true)
    assert.equal(result.markdown.match(/\| Regression \|/g).length, 10)
    assert.equal(result.markdown.match(/\| Within tolerance \|/g).length, 12)
    const cli = new URL('./compare-integrated-benchmarks.mjs', import.meta.url)
    const run = spawnSync(process.execPath, [fileURLToPath(cli), directory], { encoding: 'utf8' })
    assert.equal(run.status, 1)
    assert.match(run.stdout, /Regression/)
    assert.equal(run.stderr, '')
    for (const scenario of scenarios) {
      const input = { results: ['pacquet', 'pnpr'].flatMap(engine => [command(`${engine}@HEAD`, stable), command(`${engine}@main`, stable)]) }
      await writeFile(join(directory, `BENCHMARK_REPORT_${scenario}.json`), JSON.stringify(input))
    }
    assert.equal(spawnSync(process.execPath, [fileURLToPath(cli), directory]).status, 0)
    await rm(join(directory, `BENCHMARK_REPORT_${scenarios[0]}.json`))
    await assert.rejects(renderComparison(directory), /ENOENT/)
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
