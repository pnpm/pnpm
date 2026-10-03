import { spawnSync } from 'node:child_process'
import console from 'node:console'
import { chmod, copyFile, cp, mkdir, mkdtemp, readFile, readdir, rename, rm, writeFile } from 'node:fs/promises'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath, URL } from 'node:url'

const runtime = fileURLToPath(new URL('./', import.meta.url))
const root = path.resolve(runtime, '../..')
const target = path.join(root, 'target')
const artifact = path.join(target, 'wasm32-wasip1-threads/release/pnpm.wasm')

/**
 * Writes the publishable `@pnpm/wasm` package to `directory`, with the runtime
 * and its locked dependencies under `dist/`.
 */
export async function stagePackage (directory) {
  const cli = JSON.parse(await readFile(path.join(root, 'pnpm/npm/pnpm/package.json'), 'utf8'))
  await stageRuntime(path.join(directory, 'dist'))
  await copyFile(path.join(root, 'LICENSE'), path.join(directory, 'LICENSE'))
  await copyFile(path.join(runtime, 'PACKAGE_README.md'), path.join(directory, 'README.md'))
  const manifest = {
    name: '@pnpm/wasm',
    version: cli.version,
    description: 'pnpm for StackBlitz WebContainers',
    keywords: ['pnpm', 'webcontainer', 'wasm'],
    license: cli.license,
    homepage: cli.homepage,
    bugs: cli.bugs,
    repository: { ...cli.repository, directory: 'pnpm/wasm' },
    type: 'module',
    engines: { node: '>=22.13' },
    bin: { pnpm: 'dist/pnpm.mjs', pn: 'dist/pnpm.mjs', pnpx: 'dist/pnpx.mjs', pnx: 'dist/pnpx.mjs' },
    files: ['dist/'],
  }
  await writeFile(path.join(directory, 'package.json'), JSON.stringify(manifest, null, 2) + '\n')
}

export async function stageRuntime (directory, { bundleDependencies = true } = {}) {
  await mkdir(path.dirname(directory), { recursive: true })
  const staging = await mkdtemp(`${directory}-`)
  try {
    const dependencies = await writeRuntime(staging, bundleDependencies)
    await rm(directory, { recursive: true, force: true })
    await rename(staging, directory)
    return dependencies
  } finally {
    await rm(staging, { recursive: true, force: true })
  }
}

async function writeRuntime (directory, bundleDependencies) {
  const module = await globalThis.WebAssembly.compile(await readFile(artifact))
  if (!globalThis.WebAssembly.Module.imports(module).some(entry => entry.module === 'pnpm_atomic')) {
    throw new Error('Build the WebContainer artifact with pnpm build:pnpm:wasm before staging')
  }
  await mkdir(directory, { recursive: true })
  await Promise.all(['run.mjs', 'worker.mjs', 'pnpm.mjs'].map(file => copyFile(path.join(runtime, file), path.join(directory, file))))
  await copyFile(path.join(runtime, 'pnpm.mjs'), path.join(directory, 'pnpx.mjs'))
  await Promise.all(['pnpm.mjs', 'pnpx.mjs'].map(file => chmod(path.join(directory, file), 0o755)))
  await cp(path.join(runtime, 'host'), path.join(directory, 'host'), { recursive: true, filter: source => !source.endsWith('.test.mjs') })
  await copyFile(artifact, path.join(directory, 'pnpm.wasm'))
  await copyFile(path.join(root, 'LICENSE'), path.join(directory, 'LICENSE'))
  await copyFile(path.join(root, 'pnpm/npm/pnpm/THIRD-PARTY-NOTICES.md'), path.join(directory, 'THIRD-PARTY-NOTICES.md'))
  const dependencies = await runtimeDependencies()
  await writeFile(path.join(directory, 'package.json'), JSON.stringify({ private: true, type: 'module', dependencies }, null, 2) + '\n')
  if (bundleDependencies) await bundleRuntimeDependencies(directory)
  return dependencies
}

async function runtimeDependencies () {
  const manifest = JSON.parse(await readFile(path.join(runtime, 'package.json'), 'utf8'))
  const entries = await Promise.all(Object.keys(manifest.dependencies).map(async name => {
    const dependency = JSON.parse(await readFile(path.join(runtime, 'node_modules', name, 'package.json'), 'utf8'))
    return [name, dependency.version]
  }))
  return Object.fromEntries(entries)
}

async function bundleRuntimeDependencies (directory) {
  await mkdir(target, { recursive: true })
  const deployment = await mkdtemp(path.join(target, 'pnpm-wasm-dependencies-'))
  try {
    const result = spawnSync('pnpm', [
      '--config.inject-workspace-packages=true', '--config.node-linker=hoisted',
      '--ignore-scripts', '--filter=@pnpm-private/wasm-runtime', '--prod', 'deploy', deployment,
    ], {
      cwd: root,
      stdio: 'inherit',
      env: { ...process.env, pnpm_config_prefer_symlinked_executables: 'false' },
    })
    if (result.error) throw result.error
    if (result.status !== 0) throw new Error('Could not deploy the locked WASM runtime dependencies')
    const modules = path.join(deployment, 'node_modules')
    await rm(path.join(modules, '.pnpm'), { recursive: true, force: true })
    await rm(path.join(modules, '.modules.yaml'), { force: true })
    await validateDependencies(modules)
    await rm(path.join(directory, 'node_modules'), { recursive: true, force: true })
    await rename(modules, path.join(directory, 'node_modules'))
  } finally {
    await rm(deployment, { recursive: true, force: true })
  }
}

async function validateDependencies (directory) {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const file = path.join(directory, entry.name)
    if (entry.isSymbolicLink()) throw new Error(`The bundled WASM runtime contains a symlink: ${file}`)
    if (entry.isDirectory()) await validateDependencies(file)
    else if (entry.name.endsWith('.map')) await rm(file)
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const destination = path.join(root, 'pnpm/npm/wasm')
  await rm(destination, { recursive: true, force: true })
  await stagePackage(destination)
  console.log(destination)
}
