import fs from 'node:fs'
import path from 'node:path'
import { execSync } from 'node:child_process'

// Create dist/node_modules via pnpm deploy. Handles external runtime
// deps (node-gyp, v8-compile-cache, @reflink/reflink) that esbuild
// leaves out of the bundled pnpm.mjs. Workspace settings, patches,
// and the lockfile are carried over so pnpm.overrides (e.g. CVE fixes)
// apply.

const WORKSPACE_DIR = path.join(import.meta.dirname, '..', '..')
const DEPLOY_DIR = path.join(import.meta.dirname, 'temp-deploy')

const NODE_MODULES_TEMP_DIR = path.join(DEPLOY_DIR, 'node_modules')
const NODE_MODULES_DEST_DIR = path.join(import.meta.dirname, 'dist/node_modules')

function cleanupNodeModules (dir: string) {
  const nmPrune = path.join(import.meta.dirname, 'node_modules/.bin/nm-prune')
  execSync(`${nmPrune} --force`, { cwd: dir, stdio: 'inherit' })

  const pnpmStateFiles = [
    // Hoisted linker leaves only a small .lock.yaml — not needed in dist.
    'node_modules/.pnpm',
    'node_modules/.modules.yaml',
    'node_modules/.pnpm-workspace-state-v1.json',
  ]
  for (const file of pnpmStateFiles) {
    fs.rmSync(path.join(dir, file), { recursive: true })
  }
}

function createDistNodeModules () {
  fs.rmSync(DEPLOY_DIR, { recursive: true, force: true })

  const pnpmDeploy = [
    'pnpm',
    '--config.inject-workspace-packages=true',
    '--config.node-linker=hoisted',
    '--ignore-scripts',
    '--force',
    '--filter=pnpm',
    '--prod',
    'deploy',
    DEPLOY_DIR
  ].join(' ')
  execSync(pnpmDeploy, { cwd: WORKSPACE_DIR, stdio: 'inherit' })

  cleanupNodeModules(DEPLOY_DIR)

  fs.rmSync(NODE_MODULES_DEST_DIR, { recursive: true, force: true })
  fs.mkdirSync(path.dirname(NODE_MODULES_DEST_DIR), { recursive: true })
  fs.renameSync(NODE_MODULES_TEMP_DIR, NODE_MODULES_DEST_DIR)

  fs.rmSync(DEPLOY_DIR, { recursive: true })
}

createDistNodeModules()

// The bundled dist/node_modules already contains every runtime dependency, so
// the published manifest must not declare dependencies or devDependencies.
// The .pnpmfile.cjs beforePacking hook strips these fields when packing.
// The manifest on disk must stay untouched — stripping it here broke every
// later `pnpm install` mid-release.
