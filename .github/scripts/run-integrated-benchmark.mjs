import { spawnSync } from 'node:child_process'
import { readFile, rm, writeFile } from 'node:fs/promises'
import { join, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'
import { compare, parseSyscallSummary, syscallCountRounds } from './compare-integrated-benchmarks.mjs'

export async function runBenchmark (args, { directory = 'bench-work-env', execute = executeBenchmark } = {}) {
  const engines = ['pacquet', 'pnpr'].filter(engine => args.includes(`${engine}@HEAD`) && args.includes(`${engine}@main`))
  if (engines.length === 0) return execute(args)
  // A harness that fails before its traced runs must not leave an earlier
  // run's counts in this work env to be attached to its report.
  await removeSyscallCounts(directory, args.filter(isTarget))
  const status = await execute([...args, '--count-syscalls'])
  const reportPath = join(directory, 'BENCHMARK_REPORT.json')
  const markdownPath = join(directory, 'BENCHMARK_REPORT.md')
  if (status !== 0) {
    // The harness checks its diagnostics after the traced runs, so a failed
    // check still leaves a report and counts the workflow publishes.
    try {
      await attachSyscallCounts(reportPath, directory, args)
    } catch (error) {
      console.error(`Syscall counts not recorded after the harness exited with ${status}: ${error.message}`)
    }
    return status
  }
  const initial = await attachSyscallCounts(reportPath, directory, args)
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

async function attachSyscallCounts (reportPath, directory, args) {
  const withCounts = {
    ...JSON.parse(await readFile(reportPath, 'utf8')),
    syscalls: await readSyscallCounts(directory, args.filter(isTarget)),
  }
  await writeFile(reportPath, JSON.stringify(withCounts))
  return withCounts
}

async function removeSyscallCounts (directory, targets) {
  for (const target of targets) {
    for (let round = 0; round < syscallCountRounds; round++) {
      await rm(join(directory, target, `SYSCALL_COUNTS.${round}.txt`), { force: true })
    }
  }
}

async function readSyscallCounts (directory, targets) {
  const counts = {}
  for (const target of targets) {
    counts[target] = []
    for (let round = 0; round < syscallCountRounds; round++) {
      const path = join(directory, target, `SYSCALL_COUNTS.${round}.txt`)
      try {
        counts[target].push(parseSyscallSummary(await readFile(path, 'utf8')))
      } catch (error) {
        throw new Error(`Cannot read syscall counts from ${path}`, { cause: error })
      }
    }
  }
  return counts
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
