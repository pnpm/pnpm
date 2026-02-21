import fs from 'node:fs'
import path from 'node:path'
import { execSync } from 'node:child_process'

// Background
// ----------
//
// The published pnpm package contains a bundled node_modules directory at
// dist/node_modules.
//
//     .
//     ├── dist
//     │   ├── node_modules
//     │   │   ├── node-gyp
//     │   │   ├── v8-compile-cache
//     │   │   └── ...
//     │   └── pnpm.mjs
//     ├── ...
//     └── package.json
//
// This is used to include certain dependencies like node-gyp out of the box
// when installing pnpm.
//
// Note that most pnpm dependencies are baked into the large pnpm.mjs file by
// esbuild. This script handles other dependencies the pnpm bundle config
// declares as "external" and resolved at runtime — node-gyp, v8-compile-cache,
// and @reflink/reflink (all platform variants, installed via --force).
//
// Strategy
// --------
//
// To create dist/node_modules, we'll run a pnpm deploy and move the results
// over into the dist dir.
//
//     .
//     ├── temp-deploy
//     │   ├── ...
//     │   ├── README.md
//     │   ├── node_modules    ──────────────┐
//     │   ├── package.json                  │
//     │   ├── pnpm-lock.yaml                │
//     │   └── pnpm-workspace.yaml           │
//     └── package.json                      │
//                                           │
//     .                                     │
//     ├── dist                              │
//     │   ├── node_modules     <────────────┘
//     │   └── pnpm.mjs
//     ├── ...
//     └── package.json
//
// The pnpm deploy command should reuse workspace settings, patches, and the
// pnpm-lock.yaml. This is important to ensure settings such as pnpm.overrides
// are carried over since they might be overrides to fix CVE vulnerabilities.

const WORKSPACE_DIR = path.join(import.meta.dirname, '..')
const DEPLOY_DIR = path.join(import.meta.dirname, 'temp-deploy')

const NODE_MODULES_TEMP_DIR = path.join(DEPLOY_DIR, 'node_modules')
const NODE_MODULES_DEST_DIR = path.join(import.meta.dirname, 'dist/node_modules')

/**
 * Remove files like CHANGELOG.md, README.md, etc from node_modules to keep the
 * final distribution smaller.
 */
function cleanupNodeModules (dir: string) {
  const nmPrune = path.join(import.meta.dirname, 'node_modules/.bin/nm-prune')
  execSync(`${nmPrune} --force`, { cwd: dir, stdio: 'inherit' })

  const pnpmStateFiles = [
    // Since we're installing with --node-linker=hoisted, this directory only
    // contains a small .lock.yaml file that's not needed in the final
    // distribution.
    'node_modules/.pnpm',
    'node_modules/.modules.yaml',
    'node_modules/.pnpm-workspace-state-v1.json',
  ]
  for (const file of pnpmStateFiles) {
    fs.rmSync(path.join(dir, file), { recursive: true })
  }
}

function createDistNodeModules () {
  // Remove the target directory to ensure the results of this script are as
  // deterministic as possible and don't carry over old state.
  fs.rmSync(DEPLOY_DIR, { recursive: true, force: true })

  const pnpmDeploy = [
    'pnpm',
    '--config.inject-workspace-packages=true',
    '--config.node-linker=hoisted',
    '--ignore-scripts',
    // --force installs all optional dependencies regardless of platform, so that
    // all @reflink/reflink-* platform packages end up in dist/node_modules.
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
