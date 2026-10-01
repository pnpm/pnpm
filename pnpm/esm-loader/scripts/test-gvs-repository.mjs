import console from 'node:console'
import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'

import { installOptOuts } from './gvs-installation.mjs'
import { assertIsolated, bundleLoader } from './repository-fixture.mjs'
import { readScenario, runSuite } from './repository-scenario.mjs'

const [fixture, ...names] = process.argv.slice(2)
if (!fixture) throw new Error('Usage: test-gvs-repository.mjs <retained fixture> <package names...>')
const root = path.resolve(fixture)
assertIsolated(root)
await bundleLoader(root)
const manifest = JSON.parse(fs.readFileSync(path.join(root, '.pnpm-store.json'), 'utf8'))
const source = JSON.parse(fs.readFileSync(path.join(root, 'repository-source.json'), 'utf8'))
const installed = await installOptOuts(source, manifest, names)
for (const [id, directory] of Object.entries(installed.materialized)) {
  manifest.packages[id] = { root: directory, resolution: 'node' }
}
const manifestPath = path.join(root, '.pnpm-gvs.json')
fs.writeFileSync(manifestPath, JSON.stringify(manifest))
const result = runSuite(root, manifestPath, readScenario(root))
fs.writeFileSync(path.join(root, 'gvs.stdout'), result.stdout)
fs.writeFileSync(path.join(root, 'gvs.stderr'), result.stderr)
assertIsolated(root)
const report = { ...installed, status: result.status, noProjectNodeModules: true }
fs.writeFileSync(path.join(root, 'gvs-results.json'), JSON.stringify(report, null, 2))
console.log(JSON.stringify({ installation: installed.installation, globalVirtualStoreDir: installed.globalVirtualStoreDir, selected: installed.selected, materializedPackages: Object.keys(installed.materialized).length, status: result.status, noProjectNodeModules: true }, null, 2), result.stdout, result.stderr)
process.exitCode = result.status ?? 1
