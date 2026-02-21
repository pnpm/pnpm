import fs from 'fs'
import os from 'os'
import path from 'path'
import { sync as execaSync } from 'execa'

const NODE_VERSION = '25.6.1'
const artifactsDir = path.join(import.meta.dirname, '../..')
const pnpmDir = path.join(artifactsDir, '..')
const nodeBinCacheDir = path.join(artifactsDir, '.node-binaries')

// Map from pnpm build-sea target names to artifact directory names.
// Musl targets use the traditional "linuxstatic" naming in artifact dirs.
const SEA_TO_ARTIFACT: Record<string, string> = {
  'linux-x64': 'linux-x64',
  'linux-arm64': 'linux-arm64',
  'linux-x64-musl': 'linuxstatic-x64',
  'linux-arm64-musl': 'linuxstatic-arm64',
  'macos-x64': 'macos-x64',
  'macos-arm64': 'macos-arm64',
  'win-x64': 'win-x64',
  'win-arm64': 'win-arm64',
}

function copyDistAssets (targetDir: string): void {
  const distSrc = path.join(pnpmDir, 'dist')
  const distDest = path.join(targetDir, 'dist')

  // Remove existing dist directory
  fs.rmSync(distDest, { recursive: true, force: true })

  // Copy the dist directory
  fs.cpSync(distSrc, distDest, { recursive: true })

  // Remove source maps from the copied dist (they're archived separately)
  const removeMapFiles = (dir: string) => {
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
}

;(async () => {
  const isM1Mac = process.platform === 'darwin' && process.arch === 'arm64'
  const targets = (process.platform === 'linux' || isM1Mac)
    ? Object.keys(SEA_TO_ARTIFACT)
    : ['linux-x64', 'macos-x64', 'win-x64']

  // Build all SEA binaries into a temporary directory, then move them
  // into the platform-specific artifact directories.
  const tmpOutputDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-sea-'))
  try {
    const args = [
      'build-sea',
      '--entry', 'pnpm.cjs',
      '--output-dir', tmpOutputDir,
      '--output-name', 'pnpm',
      '--node-version', NODE_VERSION,
      ...targets.flatMap(t => ['--target', t]),
    ]
    execaSync('pnpm', args, {
      cwd: pnpmDir,
      stdio: 'inherit',
      env: { ...process.env, PNPM_HOME: nodeBinCacheDir },
    })

    // Move each binary to its artifact directory (with linuxstatic naming for musl)
    for (const seaTarget of targets) {
      const artifactTarget = SEA_TO_ARTIFACT[seaTarget]
      const ext = seaTarget.startsWith('win-') ? '.exe' : ''
      const src = path.join(tmpOutputDir, seaTarget, `pnpm${ext}`)
      const destDir = path.join(artifactsDir, artifactTarget)
      fs.mkdirSync(destDir, { recursive: true })
      fs.copyFileSync(src, path.join(destDir, `pnpm${ext}`))
      console.log(`Successfully built ${artifactTarget}`)
    }
  } finally {
    fs.rmSync(tmpOutputDir, { recursive: true, force: true })
  }

  // Copy dist/ to the exe directory for npm publishing.
  // Platform packages only contain the binary; dist/ is shipped in @pnpm/exe.
  const exeDir = path.join(artifactsDir, 'exe')
  copyDistAssets(exeDir)
  // Remove all bundled reflink packages — @pnpm/exe declares @reflink/reflink
  // as a dependency, so npm installs the right platform package automatically.
  fs.rmSync(path.join(exeDir, 'dist', 'node_modules', '@reflink'), { recursive: true, force: true })
  console.log('Copied dist/ to exe directory for npm publishing')
})().catch((err) => {
  console.error(err)
  process.exitCode = 1
})
