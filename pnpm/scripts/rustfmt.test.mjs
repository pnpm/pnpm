import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { test } from 'node:test'
import { ensureInstallation, formatterCommand, installationPath } from './rustfmt.mjs'

const config = {
  repository: 'https://github.com/pnpm/rustfmt',
  revision: '0123456789abcdef0123456789abcdef01234567',
  toolchain: 'nightly-2026-08-27',
}
const installedComponents = 'cargo-test-host\nrustc-test-host\nrustc-dev-test-host\nllvm-tools-test-host\n'

function temporaryCache (context) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-rustfmt-test-'))
  context.after(() => fs.rmSync(root, { recursive: true, force: true }))
  return root
}

function installBinaries (destination) {
  fs.mkdirSync(path.join(destination, 'bin'), { recursive: true })
  for (const name of ['rustfmt', 'cargo-fmt']) {
    fs.writeFileSync(path.join(destination, 'bin', `${name}${process.platform === 'win32' ? '.exe' : ''}`), '')
  }
}

test('builds the immutable revision outside the workspace, then reuses it', context => {
  const root = temporaryCache(context)
  const commands = []
  const run = (program, args, options) => {
    commands.push({ program, args, options })
    if (args[0] === 'component') return { status: 0, stdout: installedComponents }
    const destination = args[args.indexOf('--root') + 1]
    assert.equal(options.cwd, destination)
    assert.deepEqual(options.stdio, ['ignore', 2, 2])
    assert.equal(options.env.CARGO_TARGET_DIR, undefined)
    installBinaries(destination)
    return { status: 0 }
  }
  const destination = ensureInstallation(config, { root, run })
  assert.equal(destination, installationPath(config, root))
  assert.deepEqual(commands[1].args.slice(0, 9), ['run', config.toolchain, 'cargo', 'install',
    '--git', config.repository, '--rev', config.revision, '--locked'])
  assert.equal(ensureInstallation(config, { root, run }), destination)
  assert.equal(commands.length, 3)
  assert.ok(commands.every(command => command.program === 'rustup'))
})

test('installs runtime components even when the binary cache was restored', context => {
  const root = temporaryCache(context)
  installBinaries(installationPath(config, root))
  const commands = []
  ensureInstallation(config, {
    root,
    run: (program, args) => {
      commands.push(args)
      return args[0] === 'component' ? { status: 1, stdout: '' } : { status: 0 }
    },
  })
  assert.deepEqual(commands[1], ['toolchain', 'install', config.toolchain, '--profile', 'minimal',
    '--component', 'rustc-dev', '--component', 'llvm-tools-preview'])
  assert.equal(commands.length, 2)
})

test('installs host compiler libraries when only cross-target libraries are present', context => {
  const root = temporaryCache(context)
  installBinaries(installationPath(config, root))
  const commands = []
  ensureInstallation(config, {
    root,
    run: (program, args) => {
      commands.push(args)
      return args[0] === 'component'
        ? { status: 0, stdout: installedComponents.replace('rustc-dev-test-host', 'rustc-dev-other-host') }
        : { status: 0 }
    },
  })
  assert.equal(commands.length, 2)
  assert.deepEqual(commands[1], ['toolchain', 'install', config.toolchain, '--profile', 'minimal',
    '--component', 'rustc-dev', '--component', 'llvm-tools-preview'])
})

test('failed installations leave no cache entry and never run a fallback', context => {
  const root = temporaryCache(context)
  assert.throws(() => ensureInstallation(config, {
    root,
    run: (program, args) => args[0] === 'component'
      ? { status: 0, stdout: installedComponents }
      : { status: 17 },
  }), /failed \(17\)/)
  assert.equal(fs.existsSync(installationPath(config, root)), false)
  assert.deepEqual(fs.readdirSync(path.dirname(installationPath(config, root))), [])
})

test('a concurrent completed installation wins without replacing its binaries', context => {
  const root = temporaryCache(context)
  const destination = installationPath(config, root)
  ensureInstallation(config, {
    root,
    run: (program, args) => {
      if (args[0] === 'component') return { status: 0, stdout: installedComponents }
      installBinaries(args[args.indexOf('--root') + 1])
      installBinaries(destination)
      fs.writeFileSync(path.join(destination, 'winner'), 'first')
      return { status: 0 }
    },
  })
  assert.equal(fs.readFileSync(path.join(destination, 'winner'), 'utf8'), 'first')
  assert.deepEqual(fs.readdirSync(path.dirname(destination)), [config.revision])
})

test('forwards cargo-fmt and raw editor arguments through the pinned runtime', () => {
  const destination = installationPath(config)
  const cargo = formatterCommand(config, destination, ['--all', '--', '--check'])
  assert.deepEqual(cargo.args.slice(0, 2), ['run', config.toolchain])
  assert.deepEqual(cargo.args.slice(3), ['fmt', '--all', '--', '--check'])
  assert.match(cargo.env.RUSTFMT, /[/\\]bin[/\\]rustfmt(?:\.exe)?$/)
  const raw = formatterCommand(config, destination, ['--rustfmt', '--edition', '2024'])
  assert.equal(raw.args[2], cargo.env.RUSTFMT)
  assert.deepEqual(raw.args.slice(3), ['--edition', '2024'])
})

test('rejects moving revisions and undated toolchains before installation', () => {
  assert.throws(() => installationPath({ ...config, revision: 'main' }), /full Git commit SHA/)
  assert.throws(() => installationPath({ ...config, toolchain: 'nightly' }), /dated nightly/)
})
