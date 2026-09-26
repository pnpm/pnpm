import { spawnSync } from 'node:child_process'
import { readFile, writeFile } from 'node:fs/promises'
import { join, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'
import { compare } from './compare-integrated-benchmarks.mjs'

export async function runBenchmark (args, { directory = 'bench-work-env', execute = executeBenchmark } = {}) {
  const status = await execute(args)
  if (status !== 0) return status
  const engines = ['pacquet', 'pnpr'].filter(engine => args.includes(`${engine}@HEAD`) && args.includes(`${engine}@main`))
  if (engines.length === 0) return 0
  const reportPath = join(directory, 'BENCHMARK_REPORT.json')
  const markdownPath = join(directory, 'BENCHMARK_REPORT.md')
  const initial = JSON.parse(await readFile(reportPath, 'utf8'))
  if (!engines.some(engine => compare(initial, engine).status === 'Regression')) return 0

  const markdown = await readFile(markdownPath, 'utf8')
  const targets = args.filter(isTarget)
  const reversed = args.map(arg => isTarget(arg) ? targets.pop() : arg)
  console.log('Confirming suspected regression once, with target order reversed.')
  const confirmationStatus = await execute(reversed)
  if (confirmationStatus !== 0) {
    await writeFile(reportPath, JSON.stringify(initial))
    await writeFile(markdownPath, markdown + '\nConfirmation run failed. See the workflow log.\n')
    return confirmationStatus
  }
  const confirmation = JSON.parse(await readFile(reportPath, 'utf8'))
  const confirmationMarkdown = await readFile(markdownPath, 'utf8')
  await writeFile(reportPath, JSON.stringify({ ...initial, confirmation }))
  await writeFile(markdownPath, `${markdown}\n#### Confirmation (reversed target order)\n\n${confirmationMarkdown}`)
  return 0
}

function isTarget (arg) {
  return ['pacquet@', 'pnpr@', 'pnpm@'].some(prefix => arg.startsWith(prefix))
}

function executeBenchmark (args) {
  const result = spawnSync('target/debug/integrated-benchmark', args, { stdio: 'inherit' })
  if (result.error) throw result.error
  return result.status ?? 1
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  process.exitCode = await runBenchmark(process.argv.slice(2))
}
