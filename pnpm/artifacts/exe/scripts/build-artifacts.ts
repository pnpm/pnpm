import fs from 'node:fs'
import path from 'node:path'

import * as execa from 'execa'

// scripts/ → exe/ → artifacts/ → pnpm/
const exeDir = path.resolve(import.meta.dirname, '..')
const pnpmRootDir = path.resolve(exeDir, '..', '..')

// On Intel Mac we only build the three baseline targets to keep dev-local runs
// fast. CI (Linux) and M1 Macs produce the full eight-target matrix. The
// defaults (entry, outputDir, outputName, targets) live in the "pnpm.app"
// object of pnpm/artifacts/exe/package.json — CLI --target flags replace that
// list when we want to narrow it.
const isM1Mac = process.platform === 'darwin' && process.arch === 'arm64'
const buildFullMatrix = process.platform === 'linux' || isM1Mac

const narrowTargets = ['win32-x64', 'linux-x64', 'darwin-x64']

// Could equivalently live under `pnpm.app.runtime` in package.json; kept here
// next to the host-conditional target narrowing so the whole build matrix is
// visible in one place.
const EMBEDDED_RUNTIME = 'node@25.9.0'

const packAppArgs = ['pack-app', '--runtime', EMBEDDED_RUNTIME]
if (!buildFullMatrix) {
  for (const target of narrowTargets) {
    packAppArgs.push('--target', target)
  }
}

// Use the just-built bundle so pack-app is invoked from the same tree we're
// releasing. runPnpmCli inside pack-app forwards through process.execPath +
// argv[1], so nested `pnpm add node@runtime:<v>` calls also go through this
// bundle rather than whatever pnpm happens to be on PATH.
const pnpmBundle = path.join(pnpmRootDir, 'dist', 'pnpm.mjs')
execa.sync(process.execPath, [pnpmBundle, 'with', 'current', ...packAppArgs], {
  cwd: exeDir,
  stdio: 'inherit',
})

// Platform packages only contain the binary; the JS bundle ships inside
// @pnpm/exe. Copy it here so `pn publish` picks it up from this package's
// "files" list. Source maps are stripped (they're archived separately).
const distSrc = path.join(pnpmRootDir, 'dist')
const distDest = path.join(exeDir, 'dist')
fs.rmSync(distDest, { recursive: true, force: true })
fs.cpSync(distSrc, distDest, { recursive: true })

const removeMapFiles = (dir: string): void => {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const fullPath = path.join(dir, entry.name)
    if (entry.isDirectory()) {
      removeMapFiles(fullPath)
    } else if (entry.name.endsWith('.map')) {
      fs.unlinkSync(fullPath)
    }
  }
}
removeMapFiles(distDest)

// @pnpm/exe declares @reflink/reflink as a dependency, so npm installs the
// right platform package on the consumer. Drop the bundled copies from the
// published dist/ to avoid shipping them twice.
fs.rmSync(path.join(distDest, 'node_modules', '@reflink'), { recursive: true, force: true })

console.log('Copied dist/ to exe directory for npm publishing')
