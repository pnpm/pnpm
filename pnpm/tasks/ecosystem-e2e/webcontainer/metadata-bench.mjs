import assert from 'node:assert/strict'
import { closeSync, chmodSync, fstatSync, lstatSync, mkdtempSync, openSync, rmSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { performance } from 'node:perf_hooks'

assert.ok(process.versions.webcontainer, 'run this benchmark in a WebContainer')
console.log(`runtime: node=${process.version} webcontainer=${process.versions.webcontainer}`)

const directory = mkdtempSync(path.join(os.tmpdir(), 'pnpm-metadata-bench-'))
const files = Array.from({ length: 256 }, (_, index) => path.join(directory, `file-${index}`))
for (const file of files) writeFileSync(file, 'metadata probe')

try {
  const measurements = [
    ['lstat', file => lstatSync(file)],
    ['open-fstat-close', file => {
      const descriptor = openSync(file, 'r')
      try { fstatSync(descriptor) } finally { closeSync(descriptor) }
    }],
    ['chmod', (file, index) => chmodSync(file, index % 2 ? 0o600 : 0o640)],
  ]
  for (const [name, operation] of measurements) {
    const samples = []
    for (let round = 0; round < 30; round++) {
      const started = performance.now()
      files.forEach(operation)
      samples.push((performance.now() - started) * 1000 / files.length)
    }
    samples.sort((left, right) => left - right)
    console.log(`${name}: median=${samples[15].toFixed(2)}us/file p95=${samples[28].toFixed(2)}us/file`)
  }
  console.log('webcontainer-metadata-bench-ok')
} finally {
  rmSync(directory, { recursive: true, force: true })
}
