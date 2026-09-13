import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath, pathToFileURL } from 'node:url'

const pin = JSON.parse(fs.readFileSync(new URL('./rustfmt.json', import.meta.url), 'utf8'))
const cacheRoot = path.join(os.homedir(), '.cache', 'pnpm', 'rustfmt')

export function installationPath (config, root = cacheRoot) {
  if (!/^[a-f0-9]{40}$/.test(config.revision)) {
    throw new Error('rustfmt.json must pin a full Git commit SHA')
  }
  if (!/^nightly-\d{4}-\d{2}-\d{2}$/.test(config.toolchain)) {
    throw new Error('rustfmt.json must pin a dated nightly toolchain')
  }
  return path.join(root, `${process.platform}-${process.arch}`, config.toolchain, config.revision)
}

export function ensureInstallation (config, { root = cacheRoot, run = spawnSync } = {}) {
  const destination = installationPath(config, root)
  const components = run('rustup', ['component', 'list', '--toolchain', config.toolchain, '--installed'], {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  if (components.error != null) throw components.error
  if (components.status !== 0 || !hasRequiredComponents(components.stdout)) {
    checkedRun(run, ['toolchain', 'install', config.toolchain, '--profile', 'minimal',
      '--component', 'rustc-dev', '--component', 'llvm-tools-preview'])
  }
  if (isInstalled(destination)) return destination

  fs.mkdirSync(path.dirname(destination), { recursive: true })
  const staging = fs.mkdtempSync(`${destination}-install-`)
  try {
    // An external working directory keeps the workspace's Cargo source replacement out of this build.
    checkedRun(run, ['run', config.toolchain, 'cargo', 'install', '--git', config.repository,
      '--rev', config.revision, '--locked', '--root', staging,
      '--bin', 'rustfmt', '--bin', 'cargo-fmt', 'rustfmt-nightly'], {
      cwd: staging,
      env: buildEnvironment(),
    })
    if (!isInstalled(staging)) throw new Error('The rustfmt build did not install both formatter binaries')
    try {
      fs.renameSync(staging, destination)
    } catch (error) {
      // A concurrent invocation may have finished installing the same immutable revision first.
      if (!['EEXIST', 'ENOTEMPTY', 'EPERM'].includes(error.code) || !isInstalled(destination)) throw error
    }
  } finally {
    fs.rmSync(staging, { recursive: true, force: true })
  }
  return destination
}

export function formatterCommand (config, destination, args) {
  const raw = args[0] === '--rustfmt'
  return {
    args: ['run', config.toolchain, binaryPath(destination, raw ? 'rustfmt' : 'cargo-fmt'),
      ...(raw ? args.slice(1) : ['fmt', ...args])],
    env: { ...process.env, RUSTFMT: binaryPath(destination, 'rustfmt') },
  }
}

function hasRequiredComponents (output) {
  const components = new Set(output.trim().split(/\r?\n/))
  return [...components].some(component => {
    if (!component.startsWith('cargo-')) return false
    const host = component.slice('cargo-'.length)
    return ['rustc', 'rustc-dev', 'llvm-tools'].every(name => components.has(`${name}-${host}`))
  })
}

function binaryPath (destination, name) {
  return path.join(destination, 'bin', `${name}${process.platform === 'win32' ? '.exe' : ''}`)
}

function isInstalled (destination) {
  return ['rustfmt', 'cargo-fmt'].every(name => fs.existsSync(binaryPath(destination, name)))
}

function buildEnvironment () {
  const env = { ...process.env }
  for (const name of ['RUSTC', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER', 'RUSTFLAGS',
    'CARGO_ENCODED_RUSTFLAGS', 'CARGO_TARGET_DIR', 'CARGO_BUILD_TARGET']) {
    delete env[name]
  }
  return env
}

function checkedRun (run, args, options = {}) {
  const result = run('rustup', args, { stdio: ['ignore', 2, 2], ...options })
  if (result.error != null) throw result.error
  if (result.status !== 0) throw new Error(`rustup ${args.join(' ')} failed (${result.status ?? result.signal})`)
}

function main () {
  const args = process.argv.slice(2)
  if (args[0] === '--cache-path') {
    process.stdout.write(`${installationPath(pin)}\n`)
    return
  }
  const destination = ensureInstallation(pin)
  if (args[0] === '--install') return
  const command = formatterCommand(pin, destination, args)
  const result = spawnSync('rustup', command.args, { env: command.env, stdio: 'inherit' })
  if (result.error != null) throw result.error
  process.exitCode = result.status ?? 1
}

if (process.argv[1] && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url) {
  try {
    main()
  } catch (error) {
    process.stderr.write(`${path.basename(fileURLToPath(import.meta.url))}: ${error.message}\n`)
    process.exitCode = 1
  }
}
