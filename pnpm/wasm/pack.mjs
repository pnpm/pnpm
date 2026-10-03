import { spawnSync } from 'node:child_process'
import console from 'node:console'
import { mkdir, mkdtemp, rename, rm } from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath, URL } from 'node:url'

import { stagePackage } from './stage.mjs'

const root = fileURLToPath(new URL('../../', import.meta.url))
const target = path.join(root, 'target')
await mkdir(target, { recursive: true })
const stage = await mkdtemp(path.join(target, 'pnpm-wasm-package-'))
try {
  await stagePackage(stage)
  const result = spawnSync('pnpm', ['--config.verify-deps-before-run=false', 'pack', '--ignore-scripts', '--json'], { cwd: stage, encoding: 'utf8' })
  if (result.error) throw result.error
  if (result.status !== 0) throw new Error(`Could not pack the WASM distribution: ${result.stderr || result.stdout}`)
  const { filename } = JSON.parse(result.stdout)
  const destination = path.join(target, 'pnpm-wasm.tgz')
  await rename(path.join(stage, filename), destination)
  console.log(destination)
} finally {
  await rm(stage, { recursive: true, force: true })
}
