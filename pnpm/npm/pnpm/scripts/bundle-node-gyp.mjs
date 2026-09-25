// Builds the `dist/` payload that ships beside the native binary in the `pnpm`
// and `@pnpm/exe` wrapper packages:
//
//     pnpm/npm/pnpm
//     ├── dist
//     │   ├── node-gyp-bin       <- wrappers put on a lifecycle script's PATH
//     │   │   ├── node-gyp
//     │   │   └── node-gyp.cmd
//     │   └── node_modules
//     │       ├── get-pnpm       <- downloads the binary for bin/pnpm.mjs
//     │       └── node-gyp       <- the frozen tree, plus its dependencies
//     ├── pnpm                   <- the native binary, placed by install.js
//     └── package.json
//
// pnpm ships node-gyp so packages whose install scripts shell out to it build
// out of the box, and get-pnpm so the Corepack entry point can fetch the native
// binary Corepack does not install. Both are the wrapper's production
// dependencies, deployed by `pnpm deploy` against this repo's lockfile, so they
// are frozen and reviewed per pnpm release rather than resolved on the user's
// machine — see pnpm/crates/executor/src/bundled_node_gyp.rs for node-gyp's
// runtime half, and pnpm/npm/pnpm/bin/pnpm.mjs for get-pnpm's.
//
// The TypeScript CLI builds the same payload for its own package in
// pnpm11/pnpm/bundle-deps.ts, and both read the node-gyp version from the
// `node-gyp` catalog entry, so the two stacks ship the same node-gyp.

import { execFileSync } from 'node:child_process'
import console from 'node:console'
import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const PNPM_ROOT = path.resolve(fileURLToPath(import.meta.url), '../..')
const REPO_ROOT = path.resolve(PNPM_ROOT, '../../..')
const DEPLOY_DIR = path.join(PNPM_ROOT, 'temp-deploy')
const DIST_DIR = path.join(PNPM_ROOT, 'dist')

// The wrappers are shared with the TypeScript CLI rather than copied, so the
// two stacks cannot drift on how a script's `node-gyp` is dispatched.
const NODE_GYP_BIN_SRC = path.join(REPO_ROOT, 'pnpm11', 'pnpm', 'node-gyp-bin')

function deployNodeGyp () {
  fs.rmSync(DEPLOY_DIR, { recursive: true, force: true })

  // Reuses the workspace lockfile, patches, and overrides — overrides in
  // particular may be pinning a subdependency away from a CVE.
  execFileSync('pnpm', [
    '--config.inject-workspace-packages=true',
    '--config.node-linker=hoisted',
    '--ignore-scripts',
    '--filter=pacquet',
    '--prod',
    'deploy',
    DEPLOY_DIR,
  ], {
    cwd: REPO_ROOT,
    stdio: 'inherit',
    // The hoisted node linker turns preferSymlinkedExecutables on, which makes
    // node_modules/.bin a directory of symlinks — and a symlink cannot travel
    // inside an npm tarball, so every bin would silently disappear from the
    // published dist/node_modules/.bin. Shell shims survive packing. Passed
    // through the environment rather than as `--config.` because the release-
    // pinned pnpm ignores that flag for this setting.
    env: { ...process.env, pnpm_config_prefer_symlinked_executables: 'false' },
  })

  const nmPrune = path.join(PNPM_ROOT, 'node_modules', '.bin', 'nm-prune')
  execFileSync(nmPrune, ['--force'], { cwd: DEPLOY_DIR, stdio: 'inherit' })
  for (const stateFile of ['node_modules/.pnpm', 'node_modules/.modules.yaml']) {
    fs.rmSync(path.join(DEPLOY_DIR, stateFile), { recursive: true, force: true })
  }
  // nm-prune keeps source maps, and a dependency's are dead weight in every
  // distribution channel — they outnumber the code they map in `tar`.
  removeSourceMaps(path.join(DEPLOY_DIR, 'node_modules'))

  fs.rmSync(DIST_DIR, { recursive: true, force: true })
  fs.mkdirSync(DIST_DIR, { recursive: true })
  fs.renameSync(path.join(DEPLOY_DIR, 'node_modules'), path.join(DIST_DIR, 'node_modules'))
  fs.rmSync(DEPLOY_DIR, { recursive: true, force: true })
}

function removeSourceMaps (dir) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const fullPath = path.join(dir, entry.name)
    if (entry.isDirectory()) {
      removeSourceMaps(fullPath)
    } else if (entry.name.endsWith('.map')) {
      fs.rmSync(fullPath)
    }
  }
}

function copyNodeGypBin () {
  const dest = path.join(DIST_DIR, 'node-gyp-bin')
  fs.cpSync(NODE_GYP_BIN_SRC, dest, { recursive: true })
  // pnpm pack normalizes modes to 0644 unless publishConfig.executableFiles
  // lists the file; the mode here is what that listing preserves.
  for (const wrapper of fs.readdirSync(dest)) {
    fs.chmodSync(path.join(dest, wrapper), 0o755)
  }
}

// The payload is only useful if a lifecycle script can actually reach node-gyp
// through it, and if the Corepack entry point can reach get-pnpm. A silently
// incomplete dist/ would ship a PATH entry that resolves nothing and a pnpm
// that cannot fetch its own binary, which is worse than shipping none at all.
function verifyPayload () {
  const required = [
    path.join(DIST_DIR, 'node-gyp-bin', 'node-gyp'),
    path.join(DIST_DIR, 'node-gyp-bin', 'node-gyp.cmd'),
    path.join(DIST_DIR, 'node_modules', 'node-gyp', 'bin', 'node-gyp.js'),
    // The Corepack entry point loads its downloader from here.
    path.join(DIST_DIR, 'node_modules', 'get-pnpm', 'lib', 'index.js'),
  ]
  const missing = required.filter((file) => !fs.existsSync(file))
  if (missing.length > 0) {
    throw new Error(
      `The dist/ payload is incomplete; missing:\n${missing.map((file) => `  ${file}`).join('\n')}`
    )
  }
  // A symlink never reaches a consumer: `pnpm pack` drops it from the tarball
  // without a warning, so it would leave the same hole as a missing file.
  const symlinks = findSymlinks(DIST_DIR)
  if (symlinks.length > 0) {
    throw new Error(
      `The dist/ payload contains symlinks, which npm tarballs cannot carry:\n${symlinks.map((file) => `  ${file}`).join('\n')}`
    )
  }
}

function findSymlinks (dir) {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const fullPath = path.join(dir, entry.name)
    if (entry.isSymbolicLink()) return [fullPath]
    return entry.isDirectory() ? findSymlinks(fullPath) : []
  })
}

deployNodeGyp()
copyNodeGypBin()
verifyPayload()
console.log(`Bundled node-gyp and get-pnpm into ${DIST_DIR}`)
