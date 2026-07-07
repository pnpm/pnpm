import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'
import PATH_NAME from 'path-name'
import { writeJsonFileSync } from 'write-json-file'
import { writeYamlFileSync } from 'write-yaml-file'

import type { ExecPnpmSyncOpts } from './utils/execPnpm.js'
import { execPnpmSync, pnpmBinLocation } from './utils/index.js'

const MARKER = '=== PNPM RESOLVED BY EXEC COMMAND ==='

/**
 * Like execPnpmSync, but with the per-user state dir isolated inside the test
 * project, so the trust-on-first-use records written by pnpmExecCommand stay
 * per-test instead of leaking into the developer's real pnpm-state.json.
 * The env var is honored for the trust records (it is user-controlled);
 * the workspace-yaml `stateDir` setting deliberately is not — see
 * 'a stateDir set in pnpm-workspace.yaml cannot suppress the notice'.
 */
function execPnpmSyncIsolated (args: string[], opts?: ExecPnpmSyncOpts): ReturnType<typeof execPnpmSync> {
  return execPnpmSync(args, {
    ...opts,
    env: {
      pnpm_config_state_dir: path.resolve('state'),
      ...opts?.env,
    },
  })
}

/**
 * Create a fake "vended" pnpm executable (a Node script; cross-spawn honors
 * the shebang on every platform) and a resolver script that prints the fake
 * binary's path — standing in for an external version manager's
 * "which pnpm" tool. The fake binary prints a marker, the args it received,
 * and the PNPM_EXEC_PATH sentinel so tests can assert on the re-exec.
 */
function setupVendedPnpm (): { fakeBin: string, resolverCommand: string[] } {
  const binDir = path.resolve('vended-bin')
  fs.mkdirSync(binDir, { recursive: true })
  const fakeBin = path.join(binDir, 'pnpm')
  fs.writeFileSync(
    fakeBin,
    `#!/usr/bin/env node
console.log(${JSON.stringify(MARKER)})
console.log('args: ' + process.argv.slice(2).join(' '))
console.log('sentinel: ' + (process.env.PNPM_EXEC_PATH ?? 'unset'))
console.log('depth: ' + (process.env.PNPM_RE_EXEC_DEPTH ?? 'unset'))
`,
    { mode: 0o755 }
  )
  const resolver = path.resolve('resolve-pnpm.js')
  fs.writeFileSync(resolver, `console.log(${JSON.stringify(fakeBin)})\n`)
  return { fakeBin, resolverCommand: [process.execPath, resolver] }
}

test('re-execs into the binary printed by pnpmExecCommand, forwarding args', async () => {
  prepare()
  const { fakeBin, resolverCommand } = setupVendedPnpm()
  writeYamlFileSync('pnpm-workspace.yaml', { pnpmExecCommand: resolverCommand })

  const { status, stdout } = execPnpmSyncIsolated(['root'], { expectSuccess: true })

  expect(status).toBe(0)
  expect(stdout.toString()).toContain(MARKER)
  expect(stdout.toString()).toContain('args: root')
  // The child carries the sentinel so nested pnpm calls skip re-resolution.
  expect(stdout.toString()).toContain(`sentinel: ${fakeBin}`)
})

test('does not re-exec when the command resolves to the running binary', async () => {
  const project = prepare()
  const resolver = path.resolve('resolve-pnpm.js')
  fs.writeFileSync(resolver, `console.log(${JSON.stringify(pnpmBinLocation)})\n`)
  writeYamlFileSync('pnpm-workspace.yaml', { pnpmExecCommand: [process.execPath, resolver] })

  const { status, stdout } = execPnpmSyncIsolated(['root'], { expectSuccess: true, omitEnvDefaults: ['pnpm_config_silent'] })

  expect(status).toBe(0)
  expect(stdout.toString()).toContain(path.join(project.dir(), 'node_modules'))
})

test('skips resolution when PNPM_EXEC_PATH is already set (nested invocation)', async () => {
  const project = prepare()
  const { resolverCommand } = setupVendedPnpm()
  writeYamlFileSync('pnpm-workspace.yaml', { pnpmExecCommand: resolverCommand })

  const { status, stdout } = execPnpmSyncIsolated(['root'], {
    env: { PNPM_EXEC_PATH: pnpmBinLocation },
    expectSuccess: true,
    omitEnvDefaults: ['pnpm_config_silent'],
  })

  expect(status).toBe(0)
  // The sentinel short-circuits: no re-exec into the fake binary.
  expect(stdout.toString()).not.toContain(MARKER)
  expect(stdout.toString()).toContain(path.join(project.dir(), 'node_modules'))
})

test('fails when the command exits non-zero', async () => {
  prepare()
  const resolver = path.resolve('resolve-pnpm.js')
  fs.writeFileSync(resolver, 'process.exit(3)\n')
  writeYamlFileSync('pnpm-workspace.yaml', { pnpmExecCommand: [process.execPath, resolver] })

  const { status, stderr } = execPnpmSyncIsolated(['root'])

  expect(status).not.toBe(0)
  expect(stderr.toString()).toContain('failed with exit code 3')
})

test('fails when the command prints nothing', async () => {
  prepare()
  const resolver = path.resolve('resolve-pnpm.js')
  fs.writeFileSync(resolver, '\n')
  writeYamlFileSync('pnpm-workspace.yaml', { pnpmExecCommand: [process.execPath, resolver] })

  const { status, stderr } = execPnpmSyncIsolated(['root'])

  expect(status).not.toBe(0)
  expect(stderr.toString()).toContain('printed no path to stdout')
})

test('fails when the command prints a non-absolute path', async () => {
  prepare()
  const resolver = path.resolve('resolve-pnpm.js')
  fs.writeFileSync(resolver, 'console.log("bin/pnpm")\n')
  writeYamlFileSync('pnpm-workspace.yaml', { pnpmExecCommand: [process.execPath, resolver] })

  const { status, stderr } = execPnpmSyncIsolated(['root'])

  expect(status).not.toBe(0)
  expect(stderr.toString()).toContain('printed a non-absolute path')
})

test('fails when the command prints a path that does not exist', async () => {
  prepare()
  const resolver = path.resolve('resolve-pnpm.js')
  fs.writeFileSync(resolver, `console.log(${JSON.stringify(path.resolve('no-such-pnpm'))})\n`)
  writeYamlFileSync('pnpm-workspace.yaml', { pnpmExecCommand: [process.execPath, resolver] })

  const { status, stderr } = execPnpmSyncIsolated(['root'])

  expect(status).not.toBe(0)
  expect(stderr.toString()).toContain('printed a path that is not an existing file')
})

test('fails when the command prints a path that is a directory', async () => {
  prepare()
  const dir = path.resolve('a-directory')
  fs.mkdirSync(dir)
  const resolver = path.resolve('resolve-pnpm.js')
  fs.writeFileSync(resolver, `console.log(${JSON.stringify(dir)})\n`)
  writeYamlFileSync('pnpm-workspace.yaml', { pnpmExecCommand: [process.execPath, resolver] })

  const { status, stderr } = execPnpmSyncIsolated(['root'])

  expect(status).not.toBe(0)
  expect(stderr.toString()).toContain('printed a path that is not an existing file')
})

test('fails when the setting is not an array of strings', async () => {
  prepare()
  writeYamlFileSync('pnpm-workspace.yaml', { pnpmExecCommand: 'my-tool which-pnpm' })

  const { status, stderr } = execPnpmSyncIsolated(['root'])

  expect(status).not.toBe(0)
  expect(stderr.toString()).toContain('must be an array of non-empty strings')
})

test('skips pnpmExecCommand for --global invocations', async () => {
  prepare()
  const resolver = path.resolve('resolve-pnpm.js')
  fs.writeFileSync(resolver, 'console.error("pnpmExecCommand should not run for --global")\nprocess.exit(1)\n')
  writeYamlFileSync('pnpm-workspace.yaml', { pnpmExecCommand: [process.execPath, resolver] })

  const global = path.resolve('global')
  const pnpmHome = path.join(global, 'pnpm')
  fs.mkdirSync(global)

  const { status, stderr } = execPnpmSyncIsolated(['root', '--global'], {
    env: {
      [PATH_NAME]: `${path.join(pnpmHome, 'bin')}${path.delimiter}${process.env[PATH_NAME]!}`,
      PNPM_HOME: pnpmHome,
      XDG_DATA_HOME: global,
    },
    expectSuccess: true,
  })

  expect(status).toBe(0)
  expect(stderr.toString()).not.toContain('pnpmExecCommand should not run for --global')
})

test('a poisoned PNPM_RE_EXEC_DEPTH counts as depth 0 instead of disabling the backstop', async () => {
  prepare()
  const { resolverCommand } = setupVendedPnpm()
  writeYamlFileSync('pnpm-workspace.yaml', { pnpmExecCommand: resolverCommand })

  // NaN >= MAX must not silently pass on every level: the malformed value is
  // coerced to 0, so the child sees the well-formed depth 1.
  const poisoned = execPnpmSyncIsolated(['root'], {
    env: { PNPM_RE_EXEC_DEPTH: 'NaN' },
    expectSuccess: true,
  })
  expect(poisoned.status).toBe(0)
  expect(poisoned.stdout.toString()).toContain('depth: 1')

  // A depth at the cap trips the guard before spawning anything.
  const atMax = execPnpmSyncIsolated(['root'], {
    env: { PNPM_RE_EXEC_DEPTH: '2' },
  })
  expect(atMax.status).not.toBe(0)
  expect(atMax.stderr.toString()).toContain('re-exec depth exceeded')
  expect(atMax.stdout.toString()).not.toContain(MARKER)
})

test('control characters in the command cannot forge the notice', async () => {
  prepare()
  const resolver = path.resolve('resolve-pnpm.js')
  fs.writeFileSync(resolver, `console.log(${JSON.stringify(pnpmBinLocation)})\n`)
  // The extra argv element tries to inject a fake resolution line into the
  // trust notice via an embedded newline.
  const forgery = 'ignored-arg\nResolved to /forged/pnpm'
  writeYamlFileSync('pnpm-workspace.yaml', { pnpmExecCommand: [process.execPath, resolver, forgery] })

  const { stderr } = execPnpmSyncIsolated(['root'], { expectSuccess: true })

  expect(stderr.toString()).toContain('ignored-arg\\nResolved to /forged/pnpm')
  expect(stderr.toString()).not.toContain('\nResolved to /forged/pnpm')
})

test('pnpmExecCommand suppresses download-based version switching; the mismatch errors instead', async () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const resolver = path.resolve('resolve-pnpm.js')
  // Resolve to the running binary so no re-exec happens and the
  // packageManager check runs in this process.
  fs.writeFileSync(resolver, `console.log(${JSON.stringify(pnpmBinLocation)})\n`)
  writeYamlFileSync('pnpm-workspace.yaml', { pnpmExecCommand: [process.execPath, resolver] })
  writeJsonFileSync('package.json', {
    packageManager: 'pnpm@9.3.0',
  })

  const { status, stdout, stderr } = execPnpmSyncIsolated(['help'], { env: { PNPM_HOME: pnpmHome } })

  // Without pnpmExecCommand this would download and switch to 9.3.0. With it,
  // binary selection belongs to the command, so the mismatch is reported
  // against the resolved binary instead.
  expect(status).not.toBe(0)
  expect(stdout.toString()).not.toContain('Version 9.3.0')
  expect(stderr.toString()).toContain('This project is configured to use 9.3.0 of pnpm')
  expect(stderr.toString()).toContain('The pnpm binary was selected by the "pnpmExecCommand" setting')
})

test('devEngines.packageManager range is validated against the binary pnpmExecCommand resolved', async () => {
  prepare()
  const resolver = path.resolve('resolve-pnpm.js')
  fs.writeFileSync(resolver, `console.log(${JSON.stringify(pnpmBinLocation)})\n`)
  writeYamlFileSync('pnpm-workspace.yaml', { pnpmExecCommand: [process.execPath, resolver] })
  // A range the resolved (current) binary cannot satisfy. The mismatch errors
  // in checkPackageManager, before syncEnvLockfile would resolve the current
  // version from the registry — which keeps this test independent of whether
  // the current version is published yet.
  writeJsonFileSync('package.json', {
    devEngines: {
      packageManager: {
        name: 'pnpm',
        version: '<10',
        onFail: 'error',
      },
    },
  })

  const { status, stderr } = execPnpmSyncIsolated(['root'])

  expect(status).not.toBe(0)
  expect(stderr.toString()).toContain('This project is configured to use <10 of pnpm')
  expect(stderr.toString()).toContain('The pnpm binary was selected by the "pnpmExecCommand" setting')
})

test('a packageManager pin satisfied by the resolved binary passes the check', async () => {
  const project = prepare()
  // Query the current version before opting in to pnpmExecCommand. An exact
  // legacy pin is never persisted to the lockfile, so no registry resolution
  // happens and the test stays independent of the release cycle.
  const pnpmVersion = execPnpmSyncIsolated(['--version'], { expectSuccess: true }).stdout.toString().trim()
  const resolver = path.resolve('resolve-pnpm.js')
  fs.writeFileSync(resolver, `console.log(${JSON.stringify(pnpmBinLocation)})\n`)
  writeYamlFileSync('pnpm-workspace.yaml', { pnpmExecCommand: [process.execPath, resolver] })
  writeJsonFileSync('package.json', {
    packageManager: `pnpm@${pnpmVersion}`,
  })

  const { status, stdout } = execPnpmSyncIsolated(['root'], { expectSuccess: true, omitEnvDefaults: ['pnpm_config_silent'] })

  expect(status).toBe(0)
  expect(stdout.toString()).toContain(path.join(project.dir(), 'node_modules'))
})

test('prints a first-use notice to stderr, then stays silent while the command is unchanged', async () => {
  prepare()
  const resolver = path.resolve('resolve-pnpm.js')
  fs.writeFileSync(resolver, `console.log(${JSON.stringify(pnpmBinLocation)})\n`)
  writeYamlFileSync('pnpm-workspace.yaml', { pnpmExecCommand: [process.execPath, resolver] })

  const firstRun = execPnpmSyncIsolated(['root'], { expectSuccess: true })
  expect(firstRun.stderr.toString()).toContain('Resolving the pnpm binary with pnpmExecCommand')
  expect(firstRun.stderr.toString()).toContain(resolver)
  expect(firstRun.stderr.toString()).toContain(`Resolved to ${pnpmBinLocation}`)
  // The notice goes to stderr only: stdout stays machine-clean.
  expect(firstRun.stdout.toString()).not.toContain('Resolving the pnpm binary with pnpmExecCommand')

  const secondRun = execPnpmSyncIsolated(['root'], { expectSuccess: true })
  expect(secondRun.stderr.toString()).not.toContain('Resolving the pnpm binary with pnpmExecCommand')
  expect(secondRun.stderr.toString()).not.toContain('Resolved to')
})

test('prints a changed-command notice when pnpm-workspace.yaml is edited', async () => {
  prepare()
  const resolver = path.resolve('resolve-pnpm.js')
  const resolver2 = path.resolve('resolve-pnpm-2.js')
  fs.writeFileSync(resolver, `console.log(${JSON.stringify(pnpmBinLocation)})\n`)
  fs.writeFileSync(resolver2, `console.log(${JSON.stringify(pnpmBinLocation)})\n`)

  writeYamlFileSync('pnpm-workspace.yaml', { pnpmExecCommand: [process.execPath, resolver] })
  execPnpmSyncIsolated(['root'], { expectSuccess: true })

  writeYamlFileSync('pnpm-workspace.yaml', { pnpmExecCommand: [process.execPath, resolver2] })
  const { stderr } = execPnpmSyncIsolated(['root'], { expectSuccess: true })

  expect(stderr.toString()).toContain('The pnpmExecCommand for this workspace has changed')
  expect(stderr.toString()).toContain(`was: ${process.execPath} ${resolver}`)
  expect(stderr.toString()).toContain(`now: ${process.execPath} ${resolver2}`)
})

test('repeats the notice when the command failed, so a failing first run never records trust', async () => {
  prepare()
  const resolver = path.resolve('resolve-pnpm.js')
  fs.writeFileSync(resolver, 'process.exit(3)\n')
  writeYamlFileSync('pnpm-workspace.yaml', { pnpmExecCommand: [process.execPath, resolver] })

  const firstRun = execPnpmSyncIsolated(['root'])
  expect(firstRun.status).not.toBe(0)
  expect(firstRun.stderr.toString()).toContain('Resolving the pnpm binary with pnpmExecCommand')

  // Fix the command; because the failed run was not recorded, the notice
  // appears again on the first successful run.
  fs.writeFileSync(resolver, `console.log(${JSON.stringify(pnpmBinLocation)})\n`)
  const secondRun = execPnpmSyncIsolated(['root'], { expectSuccess: true })
  expect(secondRun.stderr.toString()).toContain('Resolving the pnpm binary with pnpmExecCommand')
})

test('a stateDir set in pnpm-workspace.yaml cannot suppress the notice', async () => {
  const project = prepare()
  const resolver = path.resolve('resolve-pnpm.js')
  fs.writeFileSync(resolver, `console.log(${JSON.stringify(pnpmBinLocation)})\n`)
  const command = [process.execPath, resolver]

  // A malicious workspace file points stateDir at a repo-controlled directory
  // pre-seeded with a trust record for its own command — the record a real
  // first run would write. If the trust lookup honored the workspace-level
  // stateDir, this would silence the first-use notice.
  const repoStateDir = path.resolve('repo-state')
  fs.mkdirSync(repoStateDir, { recursive: true })
  writeJsonFileSync(path.join(repoStateDir, 'pnpm-state.json'), {
    pnpmExecCommands: {
      [fs.realpathSync(project.dir())]: JSON.stringify(command),
    },
  })
  writeYamlFileSync('pnpm-workspace.yaml', {
    stateDir: './repo-state',
    pnpmExecCommand: command,
  })

  const { stderr } = execPnpmSyncIsolated(['root'], { expectSuccess: true })

  expect(stderr.toString()).toContain('Resolving the pnpm binary with pnpmExecCommand')
})
