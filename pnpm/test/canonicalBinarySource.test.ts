import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpmSync } from './utils/index.js'

const MARKER = '=== FAKE CANONICAL PNPM ==='

/**
 * Write a fake "canonical" pnpm executable and a .pnpmfile.mjs whose
 * getCanonicalBinaryPath hook points at it. cross-spawn honors the shebang on
 * every platform, so a single Node script works cross-platform. The fake binary
 * prints a marker (so we can detect a re-exec) and the args it received.
 */
function setupCanonicalBinaryFixture (hookBody: string): string {
  const binDir = path.resolve('canonical-bin')
  fs.mkdirSync(binDir, { recursive: true })
  const fakeBin = path.join(binDir, 'pnpm')
  fs.writeFileSync(
    fakeBin,
    `#!/usr/bin/env node
console.log(${JSON.stringify(MARKER)})
console.log('args: ' + process.argv.slice(2).join(' '))
console.log('pm_on_fail: ' + (process.env.pnpm_config_pm_on_fail ?? 'unset'))
`,
    { mode: 0o755 }
  )
  fs.writeFileSync(
    '.pnpmfile.mjs',
    `import path from 'node:path'
export const hooks = {
  getCanonicalBinaryPath: async (ctx) => { ${hookBody} },
}
`
  )
  return fakeBin
}

test('re-execs into the binary returned by the getCanonicalBinaryPath hook', async () => {
  prepare()
  const fakeBin = setupCanonicalBinaryFixture(`return ${JSON.stringify('REPLACED_BELOW')}`)
  // Return the absolute path to the fake binary from the hook.
  fs.writeFileSync(
    '.pnpmfile.mjs',
    `export const hooks = {
  getCanonicalBinaryPath: async () => ${JSON.stringify(fakeBin)},
}
`
  )
  writeYamlFileSync('pnpm-workspace.yaml', { canonicalBinarySource: 'pnpmfile' })

  const { stdout } = execPnpmSync(['root'], { omitEnvDefaults: ['pnpm_config_silent'] })

  // The fake binary ran (re-exec happened) and received the forwarded args.
  expect(stdout.toString()).toContain(MARKER)
  expect(stdout.toString()).toContain('args: root')
  // The child is told to ignore version checks so it doesn't switch again.
  expect(stdout.toString()).toContain('pm_on_fail: ignore')
})

test('does not re-exec when the hook returns null (version already matches)', async () => {
  const project = prepare()
  setupCanonicalBinaryFixture('return null')
  fs.writeFileSync(
    '.pnpmfile.mjs',
    `export const hooks = {
  getCanonicalBinaryPath: async () => null,
}
`
  )
  writeYamlFileSync('pnpm-workspace.yaml', { canonicalBinarySource: 'pnpmfile' })

  const { stdout } = execPnpmSync(['root'], { omitEnvDefaults: ['pnpm_config_silent'] })

  // No re-exec: the running pnpm handled the command itself.
  expect(stdout.toString()).not.toContain(MARKER)
  expect(stdout.toString()).toContain(path.join(project.dir(), 'node_modules'))
})

test('errors when canonicalBinarySource is "pnpmfile" but no hook is defined', async () => {
  prepare()
  fs.writeFileSync('.pnpmfile.mjs', 'export const hooks = {}\n')
  writeYamlFileSync('pnpm-workspace.yaml', { canonicalBinarySource: 'pnpmfile' })

  const { status, stderr } = execPnpmSync(['root'], { omitEnvDefaults: ['pnpm_config_silent'] })

  expect(status).not.toBe(0)
  expect(stderr.toString()).toContain('no "getCanonicalBinaryPath" hook was found')
})

test('warns about an orphan getCanonicalBinaryPath hook when the setting is unset', async () => {
  prepare()
  setupCanonicalBinaryFixture(`return ${JSON.stringify('/nonexistent/pnpm')}`)
  // Note: no canonicalBinarySource setting in pnpm-workspace.yaml.

  const { status, stdout } = execPnpmSync(['root'], { omitEnvDefaults: ['pnpm_config_silent'] })

  // The hook is ignored: pnpm warns (via the reporter, on stdout), runs
  // normally, and never re-execs.
  expect(status).toBe(0)
  expect(stdout.toString()).toContain('getCanonicalBinaryPath')
  expect(stdout.toString()).not.toContain(MARKER)
})
