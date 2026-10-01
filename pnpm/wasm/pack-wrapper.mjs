import { spawnSync } from 'node:child_process'
import console from 'node:console'
import { copyFile, cp, mkdir, mkdtemp, readFile, rename, rm, stat, writeFile } from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath, URL } from 'node:url'

import { createWrapperManifest, WRAPPER_FILES } from '../npm/pnpm/scripts/generate-packages.mjs'
import { stageRuntime } from './stage.mjs'

const root = fileURLToPath(new URL('../../', import.meta.url))
const wrapper = path.join(root, 'pnpm/npm/pnpm')
const target = path.join(root, 'target')
await mkdir(target, { recursive: true })
const directory = await mkdtemp(path.join(target, 'pnpm-wrapper-package-'))
try {
  await stageWrapper(directory)
  const result = spawnSync('pnpm', ['--config.verify-deps-before-run=false', 'pack', '--ignore-scripts', '--json'], { cwd: directory, encoding: 'utf8' })
  if (result.error) throw result.error
  if (result.status !== 0) throw new Error(`Could not pack the pnpm wrapper: ${result.stderr}`)
  const { filename } = JSON.parse(result.stdout)
  const destination = path.join(target, 'pnpm-webcontainer-wrapper.tgz')
  await rename(path.join(directory, filename), destination)
  console.log(destination)
} finally {
  await rm(directory, { recursive: true, force: true })
}

async function stageWrapper (directory) {
  for (const file of WRAPPER_FILES) {
    await mkdir(path.dirname(path.join(directory, file)), { recursive: true })
    await copyFile(path.join(wrapper, file), path.join(directory, file))
  }
  const dist = path.join(wrapper, 'dist')
  if (await stat(dist).catch(error => {
    if (error.code !== 'ENOENT') throw error
  })) await cp(dist, path.join(directory, 'dist'), { recursive: true })
  await stageRuntime(path.join(directory, 'dist/wasm'))
  const source = JSON.parse(await readFile(path.join(wrapper, 'package.json'), 'utf8'))
  const manifest = createWrapperManifest(source)
  manifest.name = manifest.publishConfig.name
  delete manifest.publishConfig.name
  await writeFile(path.join(directory, 'package.json'), JSON.stringify(manifest, null, 2) + '\n')
}
