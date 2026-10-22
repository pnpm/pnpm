import * as execa from 'execa'
import fs from 'fs'
import os from 'os'
import path from 'path'

const artifactsDir = path.join(import.meta.dirname, '../..')

function build (target: string): string {
  let artifactFile = path.join(artifactsDir, target, 'pnpm')
  if (target.startsWith('win-')) {
    artifactFile += '.exe'
  }
  try {
    fs.unlinkSync(artifactFile)
  } catch {}
  execa.sync('pkg', ['../../dist/pnpm.cjs', `--config=../../package-${target}.json`], {
    cwd: path.join(import.meta.dirname, '..'),
    stdio: 'inherit',
  })
  // Verifying that the artifact was created.
  fs.statSync(artifactFile);
  return artifactFile
}

// The executables must not embed an older Node.js than the one this script
// runs on, which @pnpm/exe pins as its `node` devDependency. pkg-fetch decides
// the embedded version, and a lagging pkg-fetch release would otherwise ship
// an unpatched runtime silently.
function verifyEmbeddedNode (artifactFile: string): void {
  const embedded = execa.sync(artifactFile, ['-p', 'process.versions.node'], {
    env: { PKG_EXECPATH: 'PKG_INVOKE_NODEJS' },
  }).stdout.trim()
  if (compareVersions(embedded, process.versions.node) < 0) {
    throw new Error(`${artifactFile} embeds Node.js ${embedded}, older than the required ${process.versions.node}`)
  }
  console.log(`${artifactFile} embeds Node.js ${embedded}`)
}

// pnpm's patch to pkg stops the executable from replacing `node` with itself
// when it spawns a child process. Scripts must run on the system Node.js.
function verifySpawnsSystemNode (artifactFile: string): void {
  const projectDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-exe-'))
  try {
    fs.writeFileSync(path.join(projectDir, 'package.json'), '{}')
    const nodePath = execa.sync(artifactFile, ['exec', 'node', '-p', 'process.execPath'], {
      cwd: projectDir,
    }).stdout.trim().split(/\r?\n/).pop()!
    if (path.resolve(nodePath) === path.resolve(artifactFile)) {
      throw new Error(`${artifactFile} ran itself instead of the system Node.js`)
    }
  } finally {
    fs.rmSync(projectDir, { recursive: true, force: true })
  }
}

function compareVersions (a: string, b: string): number {
  const partsA = a.split('.').map(Number)
  const partsB = b.split('.').map(Number)
  for (let i = 0; i < 3; i++) {
    if (partsA[i] !== partsB[i]) return partsA[i] - partsB[i]
  }
  return 0
}

const runnableTargetsByHost: Record<string, string[]> = {
  'linux-x64': ['linux-x64', 'linuxstatic-x64'],
  'linux-arm64': ['linux-arm64', 'linuxstatic-arm64'],
  'darwin-x64': ['macos-x64'],
  'darwin-arm64': ['macos-arm64'],
  'win32-x64': ['win-x64'],
  'win32-arm64': ['win-arm64'],
}
const runnableTargets = runnableTargetsByHost[`${process.platform}-${process.arch}`] ?? []

const artifacts = new Map<string, string>()
for (const target of ['win-x64', 'linux-x64', 'linuxstatic-x64', 'macos-x64']) {
  artifacts.set(target, build(target))
}

const isM1Mac = process.platform === 'darwin' && process.arch === 'arm64'
if (process.platform === 'linux' || isM1Mac) {
  for (const target of ['macos-arm64', 'linux-arm64', 'linuxstatic-arm64', 'win-arm64']) {
    artifacts.set(target, build(target))
  }
}

for (const target of runnableTargets) {
  const artifactFile = artifacts.get(target)
  if (artifactFile == null) continue
  verifyEmbeddedNode(artifactFile)
  verifySpawnsSystemNode(artifactFile)
}
