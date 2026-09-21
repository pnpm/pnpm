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
  'ISOLATED_FRESH_RESTORE_COLD_CACHE_COLD_STORE_COLD_PNPR',
  'GVS_FRESH_RESTORE_HOT_CACHE_HOT_STORE',
]

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
    'HEAD and main are measured on the same runner. The tolerance is 5% of the main median or 2 ms, whichever is larger. A regression requires both the median slowdown and the gap between the fastest retained HEAD sample and slowest retained main sample to exceed this tolerance, after trimming up to 10% from each tail. This conservative noise guard is not a statistical confidence interval.',
    '',
    'Inconclusive rows do not fail the check. They remain visible for performance review. Bencher retains absolute minimum timings for historical tracking; historical alerts do not gate this PR. Sequential measurements can still be affected by runner drift.',
    '',
    '| Scenario | Engine | main median | HEAD median | Change | Tolerance | Result |',
    '| --- | --- | ---: | ---: | ---: | ---: | --- |',
  ]
  let failed = false
  for (const scenario of scenarios) {
    const report = JSON.parse(await readFile(join(directory, `BENCHMARK_REPORT_${scenario}.json`), 'utf8'))
    const engines = scenario.includes('PEER_HEAVY') || scenario.includes('LINKED_WORKSPACE') ? ['pacquet'] : ['pacquet', 'pnpr']
    for (const engine of engines) {
      const result = compare(report, engine)
      failed ||= result.status === 'Regression'
      lines.push(`| ${scenario.toLowerCase().replaceAll('_', '-')} | ${engine} | ${(result.base * 1000).toFixed(2)} ms | ${(result.head * 1000).toFixed(2)} ms | ${((result.ratio - 1) * 100).toFixed(1)}% | ${(result.tolerance * 1000).toFixed(2)} ms | ${result.status} |`)
    }
  }
  return { markdown: lines.join('\n') + '\n', failed }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const { markdown, failed } = await renderComparison(process.argv[2])
  console.log(markdown)
  process.exitCode = failed ? 1 : 0
}
