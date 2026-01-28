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
// Note that most pnpm dependencies baked into the large pnpm.mjs file by
// esbuild. This script handles other dependencies the pnpm bundle config
// declares as "external" and resolved at runtime. At the time of writing
// (January 2026), this is just node-gyp and v8-compile-cache. The exact list of
// bundled dependencies will likely change in the future.
//
// Strategy
// --------
//
// To create dist/node_modules, we'll run a separate pnpm install with
// --node-linker=hoisted in a temporary directory and move the results over into
// the dist dir.
//
//     .
//     ├── temp-workspace
//     │   ├── __patches__
//     │   ├── node_modules    ──────────────┐
//     │   ├── pnpm                          │
//     │   │   └── package.json              │
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
// The temporary directory needs workspace state files such as pnpm-lock.yaml
// and pnpm-workspace.yaml copied over during installation. The install in the
// temporary directory should be as similar as possible to the original
// workspace. This is important to ensure settings such as pnpm.overrides are
// carried over since they might be overrides to fix CVE vulnerabilities.

const WORKSPACE_DIR = path.join(import.meta.dirname, '..')
const TEMP_DIR = path.join(import.meta.dirname, 'temp-workspace')

const NODE_MODULES_TEMP_DIR = path.join(TEMP_DIR, 'node_modules')
const NODE_MODULES_DEST_DIR = path.join(import.meta.dirname, 'dist/node_modules')

const filesToCopy = [
  '__patches__',
  'pnpm-lock.yaml',
  'pnpm-workspace.yaml',
  'pnpm/package.json',
]

/**
 * Copy files mirroring directory structure.
 */
async function copyFiles (source: string, target: string, files: readonly string[]) {
  for (const fileOrDir of files) {
    const destinationPath = path.join(target, fileOrDir)

    await fs.promises.mkdir(path.dirname(destinationPath), { recursive: true })
    await fs.promises.cp(
      path.join(source, fileOrDir),
      destinationPath,
      { force: true, recursive: true })
  }
}

async function patchPackageManifest () {
  const filePath = path.join(TEMP_DIR, 'pnpm/package.json')
  const rawManifest = await fs.promises.readFile(filePath, 'utf8')
  const manifest = JSON.parse(rawManifest)

  // Only non-dev dependencies should be included in the bundled node_modules
  // dir. The devDependencies block also needs to be removed since it contains
  // workspace: protocol dependencies that won't resolve in the temporary
  // directory.
  delete manifest.devDependencies

  await fs.promises.writeFile(filePath, JSON.stringify(manifest, null, 2))
}

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

async function createDistNodeModules () {
  // Remove the target directory to ensure the results of this script are as
  // deterministic as possible and don't carry over old state.
  await fs.promises.rm(TEMP_DIR, { recursive: true, force: true })

  await copyFiles(WORKSPACE_DIR, TEMP_DIR, filesToCopy)
  await patchPackageManifest()

  const pnpmInstallCommand = [
    'pnpm install',
    '--node-linker=hoisted',
    '--ignore-scripts',
    // Since we're only copying the pnpm package over, the lockfile is expected
    // to be out-of-date.
    '--no-frozen-lockfile',
    // It's expected that most pnpm patches won't be used since we're only
    // installing a small subset of dependencies.
    '--config.allow-unused-patches',
  ].join(' ')
  execSync(pnpmInstallCommand, { cwd: TEMP_DIR, stdio: 'inherit' })

  cleanupNodeModules(TEMP_DIR)

  await fs.promises.rm(NODE_MODULES_DEST_DIR, { recursive: true, force: true })
  await fs.promises.mkdir(path.dirname(NODE_MODULES_DEST_DIR), { recursive: true })
  await fs.promises.rename(NODE_MODULES_TEMP_DIR, NODE_MODULES_DEST_DIR)

  await fs.promises.rm(TEMP_DIR, { recursive: true })
}

await createDistNodeModules()
