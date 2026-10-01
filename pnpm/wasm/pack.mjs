import { spawnSync } from 'node:child_process'
import { chmod, copyFile, cp, mkdir, mkdtemp, readFile, rename, rm, writeFile } from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const runtime = fileURLToPath(new URL('./', import.meta.url))
const root = path.resolve(runtime, '../..')
const target = path.join(root, 'target')
const artifact = path.join(target, 'wasm32-wasip1-threads/release/pnpm.wasm')
const module = await WebAssembly.compile(await readFile(artifact))
if (!WebAssembly.Module.imports(module).some(entry => entry.module === 'pnpm_atomic')) {
  throw new Error('Build the WebContainer artifact with pnpm build:pnpm:wasm before packing')
}
await mkdir(target, { recursive: true })
const stage = await mkdtemp(path.join(target, 'pnpm-wasm-package-'))
try {
  await stagePackage(stage)
  const result = spawnSync('npm', ['pack', '--ignore-scripts', '--json'], { cwd: stage, encoding: 'utf8' })
  if (result.error) throw result.error
  if (result.status !== 0) throw new Error(`Could not pack the WASM distribution: ${result.stderr}`)
  const [{ filename }] = Object.values(JSON.parse(result.stdout))
  const destination = path.join(target, 'pnpm-wasm.tgz')
  await rename(path.join(stage, filename), destination)
  console.log(destination)
} finally {
  await rm(stage, { recursive: true, force: true })
}

async function stagePackage (directory) {
  const cli = JSON.parse(await readFile(path.join(root, 'pnpm/npm/pnpm/package.json'), 'utf8'))
  const runtimeManifest = JSON.parse(await readFile(path.join(runtime, 'package.json'), 'utf8'))
  const manifest = {
    name: 'pnpm', version: cli.version, type: 'module', license: cli.license,
    description: 'pnpm for StackBlitz WebContainers',
    repository: cli.repository,
    engines: { node: '>=22.13' },
    bin: { pnpm: 'pnpm.mjs', pn: 'pnpm.mjs', pnpx: 'pnpx.mjs', pnx: 'pnpx.mjs' },
    dependencies: runtimeManifest.dependencies,
  }
  await writeFile(path.join(directory, 'package.json'), JSON.stringify(manifest, null, 2) + '\n')
  await Promise.all(['run.mjs', 'worker.mjs', 'pnpm.mjs'].map(file => copyFile(path.join(runtime, file), path.join(directory, file))))
  await copyFile(path.join(runtime, 'pnpm.mjs'), path.join(directory, 'pnpx.mjs'))
  await Promise.all(['pnpm.mjs', 'pnpx.mjs'].map(file => chmod(path.join(directory, file), 0o755)))
  await cp(path.join(runtime, 'host'), path.join(directory, 'host'), { recursive: true, filter: source => !source.endsWith('.test.mjs') })
  await copyFile(artifact, path.join(directory, 'pnpm.wasm'))
  await copyFile(path.join(root, 'LICENSE'), path.join(directory, 'LICENSE'))
  await copyFile(path.join(root, 'pnpm/npm/pnpm/THIRD-PARTY-NOTICES.md'), path.join(directory, 'THIRD-PARTY-NOTICES.md'))
  await copyFile(path.join(runtime, 'README.md'), path.join(directory, 'README.md'))
}
