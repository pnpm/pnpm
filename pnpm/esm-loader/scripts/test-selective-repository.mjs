import { spawnSync } from 'node:child_process'
import console from 'node:console'
import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { pathToFileURL } from 'node:url'

import { openStore } from '../store.mjs'
import { assertIsolated } from './repository-fixture.mjs'

if (!process.argv[2]) throw new Error('Pass the retained fixture path from test-repository.mjs')
const root = path.resolve(process.argv[2])
const manifestURL = pathToFileURL(path.join(root, '.pnpm-store.json'))
const manifest = JSON.parse(fs.readFileSync(manifestURL, 'utf8'))
const store = openStore(manifestURL)
const manifestPath = path.join(root, '.pnpm-selective.json')
const materialized = []
const attempts = []
let materializedBytes = 0
let materializedFiles = 0
let status = 1
assertIsolated(root)
for (let attempt = 0; attempt <= store.packages.size; attempt++) {
  fs.writeFileSync(manifestPath, JSON.stringify(manifest))
  const result = runParser(root, manifestPath)
  status = result.status ?? 1
  fs.writeFileSync(path.join(root, `selective-${attempt}.stderr`), result.stderr)
  const missingPackage = findMissingPackage(result.stderr, store)
  attempts.push({ status, missingPackage: missingPackage?.id })
  if (status === 0 || !missingPackage || materialized.includes(missingPackage.id)) {
    console.log(result.stderr)
    break
  }
  const stats = materialize(root, store, missingPackage)
  manifest.packages[missingPackage.id] = { root: stats.directory, dependencies: Object.fromEntries(missingPackage.dependencies) }
  materialized.push(missingPackage.id)
  materializedFiles += stats.files
  materializedBytes += stats.bytes
  console.log(`Materialized ${missingPackage.id}`)
}
assertIsolated(root)
const report = { status, noNodeModules: true, materialized, materializedFiles, materializedBytes, attempts }
fs.writeFileSync(path.join(root, 'selective-results.json'), JSON.stringify(report, null, 2))
console.log(`${materialized.length} packages, ${materializedFiles} files, ${(materializedBytes / 1024 / 1024).toFixed(1)} MiB materialized; exit ${status}`)
process.exitCode = status

function runParser (root, manifestPath) {
  const result = spawnSync(process.execPath, [
    '--import', pathToFileURL(path.join(root, 'loader.mjs')).href, '--experimental-vm-modules',
    path.join(root, 'repo/run-jest.cjs'), '--runInBand', '--no-cache', '--coverage=false', '--runTestsByPath', 'test/index.ts',
  ], {
    cwd: path.join(root, 'repo/pnpm11/cli/parse-cli-args'), encoding: 'utf8', timeout: 120000,
    env: { ...process.env, NODE_PATH: '', NODE_OPTIONS: '', PNPM_LOADER_MANIFEST: manifestPath, UNRS_RESOLVER_NODE_RESOLUTION: '1' },
  })
  if (result.error) throw result.error
  return result
}

function findMissingPackage (stderr, store) {
  const line = stderr.split('\n').find(line => line.includes('ENOENT') && line.includes(store.virtualRoot + path.sep))
  if (!line) return undefined
  return [...store.packages.values()].find(pkg => pkg.stored && line.includes(pkg.root + path.sep))
}

function materialize (root, store, pkg) {
  const directory = path.join('materialized', path.basename(pkg.root))
  let bytes = 0
  for (const [name, hash] of Object.entries(pkg.files)) {
    const source = store.filesystem.readFileSync(path.join(pkg.root, name))
    const filename = path.join(root, directory, name)
    fs.mkdirSync(path.dirname(filename), { recursive: true })
    fs.writeFileSync(filename, source)
    fs.chmodSync(filename, hash.endsWith('-exec') ? 0o755 : 0o644)
    bytes += source.length
  }
  return { directory, files: Object.keys(pkg.files).length, bytes }
}
