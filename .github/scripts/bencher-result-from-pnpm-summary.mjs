#!/usr/bin/env node
import { mkdir, readFile, writeFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { resolveBenchOutputPath } from './bench-output-path.mjs'

const { allowMissing, name, output, packageDir, summary } = parseArgs(process.argv.slice(2))
const summaryPath = resolve(summary)
const outputPath = resolveBenchOutputPath(output)
const packagePath = resolve(packageDir)
const { executionStatus } = JSON.parse(await readFile(summaryPath, 'utf8'))

const entry = executionStatus?.[packagePath]
if (entry == null) {
  if (allowMissing) {
    process.exit(0)
  }
  console.error(`No execution summary entry found for ${packagePath}`)
  process.exit(1)
}
if (entry.status !== 'passed') {
  console.error(`Execution summary entry for ${packagePath} has status ${entry.status}`)
  process.exit(1)
}
if (typeof entry.duration !== 'number') {
  console.error(`Execution summary entry for ${packagePath} does not include a duration`)
  process.exit(1)
}

const durationSeconds = entry.duration / 1000

await mkdir(dirname(outputPath), { recursive: true })
await writeFile(outputPath, JSON.stringify({
  results: [
    {
      command: name,
      mean: durationSeconds,
      stddev: 0,
      median: durationSeconds,
      user: 0,
      system: 0,
      min: durationSeconds,
      max: durationSeconds,
      times: [durationSeconds],
      exit_codes: [0],
    },
  ],
}, null, 2) + '\n')

function parseArgs (args) {
  let name
  let output
  let packageDir
  let summary = 'pnpm-exec-summary.json'
  let allowMissing = false

  for (let i = 0; i < args.length; i++) {
    const arg = args[i]
    if (arg === '--name') {
      name = args[++i]
    } else if (arg === '--output') {
      output = args[++i]
    } else if (arg === '--package-dir') {
      packageDir = args[++i]
    } else if (arg === '--summary') {
      summary = args[++i]
    } else if (arg === '--allow-missing') {
      allowMissing = true
    } else {
      usage(`unknown argument: ${arg}`)
    }
  }

  if (!name) usage('missing --name')
  if (!output) usage('missing --output')
  if (!packageDir) usage('missing --package-dir')

  return { allowMissing, name, output, packageDir, summary }
}

function usage (message) {
  console.error(message)
  console.error('Usage: bencher-result-from-pnpm-summary.mjs --name <benchmark> --output <file> --package-dir <dir> [--summary <file>] [--allow-missing]')
  process.exit(1)
}
