import * as execa from 'execa'
import fs from 'fs'
import path from 'path'

const NODE_VERSION = '25.6.1'
const artifactsDir = path.join(import.meta.dirname, '../..')
const pnpmDir = path.join(artifactsDir, '..')
const nodeBinCacheDir = path.join(artifactsDir, '.node-binaries')

interface TargetConfig {
  platform: string
  arch: string
  libc?: string
  /** Whether this target needs ldid signing (macOS cross-compiled from Linux) */
  needsLdidSigning: boolean
}

function getTargets (): Record<string, TargetConfig> {
  return {
    'linux-x64': { platform: 'linux', arch: 'x64', needsLdidSigning: false },
    'linux-arm64': { platform: 'linux', arch: 'arm64', needsLdidSigning: false },
    'linuxstatic-x64': { platform: 'linux', arch: 'x64', libc: 'musl', needsLdidSigning: false },
    'linuxstatic-arm64': { platform: 'linux', arch: 'arm64', libc: 'musl', needsLdidSigning: false },
    'macos-x64': { platform: 'darwin', arch: 'x64', needsLdidSigning: process.platform === 'linux' },
    'macos-arm64': { platform: 'darwin', arch: 'arm64', needsLdidSigning: process.platform === 'linux' },
    'win-x64': { platform: 'win32', arch: 'x64', needsLdidSigning: false },
    'win-arm64': { platform: 'win32', arch: 'arm64', needsLdidSigning: false },
  }
}

async function downloadNodeBinary (target: string, config: TargetConfig): Promise<string> {
  console.log(`Fetching Node.js ${NODE_VERSION} for ${target}...`)
  const args = ['env', 'add', '--global', '--json', '--platform', config.platform, '--arch', config.arch]
  if (config.libc) args.push('--libc', config.libc)
  args.push(NODE_VERSION)

  const { stdout } = execa.sync('pnpm', args, {
    stdio: ['inherit', 'pipe', 'inherit'],
    env: { ...process.env, PNPM_HOME: nodeBinCacheDir },
  })

  const [{ dir }] = JSON.parse(stdout) as Array<{ dir: string }>
  return config.platform === 'win32'
    ? path.join(dir, 'node.exe')
    : path.join(dir, 'bin', 'node')
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

async function build (target: string, config: TargetConfig): Promise<void> {
  const targetDir = path.join(artifactsDir, target)
  let artifactFile = path.join(targetDir, 'pnpm')
  if (target.startsWith('win-')) {
    artifactFile += '.exe'
  }

  // Clean up existing artifact
  try { fs.unlinkSync(artifactFile) } catch {}
  fs.mkdirSync(targetDir, { recursive: true })

  // Download the Node.js binary for this platform
  const nodeBin = await downloadNodeBinary(target, config)

  // Generate SEA config for this target
  const seaConfig = {
    main: path.join(pnpmDir, 'pnpm.cjs'),
    executable: nodeBin,
    output: artifactFile,
    disableExperimentalSEAWarning: true,
    useCodeCache: false,
    useSnapshot: false,
  }
  const seaConfigPath = path.join(targetDir, 'sea-config.json')
  fs.writeFileSync(seaConfigPath, JSON.stringify(seaConfig, null, 2))

  // Build the SEA
  console.log(`Building SEA for ${target}...`)
  execa.sync('node', ['--build-sea', seaConfigPath], {
    stdio: 'inherit',
  })

  // Clean up config
  fs.unlinkSync(seaConfigPath)

  // Sign macOS binaries
  if (config.needsLdidSigning) {
    console.log(`Signing macOS binary for ${target} with ldid...`)
    execa.sync('ldid', ['-S', artifactFile], { stdio: 'inherit' })
  } else if (config.platform === 'darwin' && process.platform === 'darwin') {
    console.log(`Signing macOS binary for ${target} with codesign...`)
    execa.sync('codesign', ['--sign', '-', artifactFile], { stdio: 'inherit' })
  }

  // Verifying that the artifact was created.
  fs.statSync(artifactFile)
  console.log(`Successfully built ${target}`)
}

;(async () => {
  const targets = getTargets()

  await build('win-x64', targets['win-x64'])
  await build('linux-x64', targets['linux-x64'])
  await build('macos-x64', targets['macos-x64'])

  const isM1Mac = process.platform === 'darwin' && process.arch === 'arm64'
  if (process.platform === 'linux' || isM1Mac) {
    await build('macos-arm64', targets['macos-arm64'])
    await build('linux-arm64', targets['linux-arm64'])
    await build('win-arm64', targets['win-arm64'])
    await build('linuxstatic-x64', targets['linuxstatic-x64'])
    await build('linuxstatic-arm64', targets['linuxstatic-arm64'])
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
