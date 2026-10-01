import { spawnSync } from 'node:child_process'
import console from 'node:console'
import { mkdir, mkdtemp, readFile, rename, rm, writeFile } from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath, URL } from 'node:url'

import { stageRuntime } from './stage.mjs'

const runtime = fileURLToPath(new URL('./', import.meta.url))
const root = path.resolve(runtime, '../..')
const target = path.join(root, 'target')
await mkdir(target, { recursive: true })
const stage = await mkdtemp(path.join(target, 'pnpm-wasm-package-'))
try {
  await stagePackage(stage)
  const result = spawnSync('pnpm', ['--config.verify-deps-before-run=false', '--config.node-linker=hoisted', 'pack', '--ignore-scripts', '--json'], { cwd: stage, encoding: 'utf8' })
  if (result.error) throw result.error
  if (result.status !== 0) throw new Error(`Could not pack the WASM distribution: ${result.stderr || result.stdout}`)
  const { filename } = JSON.parse(result.stdout)
  const destination = path.join(target, 'pnpm-wasm.tgz')
  await rename(path.join(stage, filename), destination)
  console.log(destination)
} finally {
  await rm(stage, { recursive: true, force: true })
}

async function stagePackage (directory) {
  const cli = JSON.parse(await readFile(path.join(root, 'pnpm/npm/pnpm/package.json'), 'utf8'))
  const dependencies = await stageRuntime(directory)
  const manifest = {
    name: 'pnpm', version: cli.version, type: 'module', license: cli.license,
    description: 'pnpm for StackBlitz WebContainers',
    repository: cli.repository,
    engines: { node: '>=22.13' },
    bin: { pnpm: 'pnpm.mjs', pn: 'pnpm.mjs', pnpx: 'pnpx.mjs', pnx: 'pnpx.mjs' },
    dependencies,
    bundledDependencies: Object.keys(dependencies),
  }
  await writeFile(path.join(directory, 'package.json'), JSON.stringify(manifest, null, 2) + '\n')
}
