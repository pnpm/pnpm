#!/usr/bin/env node
import { mkdir, readFile, writeFile } from 'node:fs/promises'
import { dirname } from 'node:path'
import { resolveBenchOutputPath } from './bench-output-path.mjs'

const { output, files } = parseArgs(process.argv.slice(2))
const outputPath = resolveBenchOutputPath(output)
const results = []

for (const file of files) {
  const report = JSON.parse(await readFile(file, 'utf8'))
  if (Array.isArray(report.results)) {
    results.push(...report.results)
  }
}

if (results.length === 0) {
  console.error('No Bencher results found to merge')
  process.exit(1)
}

await mkdir(dirname(outputPath), { recursive: true })
await writeFile(outputPath, JSON.stringify({ results }, null, 2) + '\n')

function parseArgs (args) {
  let output
  const filesIndex = args.indexOf('--')

  if (filesIndex === -1) {
    usage('missing file separator: --')
  }

  for (let i = 0; i < filesIndex; i++) {
    const arg = args[i]
    if (arg === '--output') {
      output = args[++i]
    } else {
      usage(`unknown argument: ${arg}`)
    }
  }

  const files = args.slice(filesIndex + 1)
  if (!output) usage('missing --output')
  if (files.length === 0) usage('missing files')

  return { output, files }
}

function usage (message) {
  console.error(message)
  console.error('Usage: merge-bencher-results.mjs --output <file> -- <file> [file...]')
  process.exit(1)
}
