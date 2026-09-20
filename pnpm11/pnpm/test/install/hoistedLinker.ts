import fs from 'node:fs'

import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'

import { execPnpmSync } from '../utils/index.js'

function runHoisted (args: string[]): string {
  return execPnpmSync([...args, '--config.node-linker=hoisted'], {
    env: { pnpm_config_silent: 'false' },
    stdio: 'pipe',
    expectSuccess: true,
  }).stdout.toString()
}

/** The reporter's summary block, or `undefined` when it printed none. */
function summary (output: string): string | undefined {
  return /^dependencies:\n(?:[+-].*\n?)+/m.exec(output)?.[0].trim()
}

test('the hoisted linker reports the version a dependency resolved to', async () => {
  prepare()

  // The manifest keeps the range the dependency was asked for. The summary
  // has to name the version behind it (pnpm/pnpm#15161).
  expect(summary(runHoisted(['add', '@pnpm.e2e/foo@^100.0.0'])))
    .toBe('dependencies:\n+ @pnpm.e2e/foo 100.1.0')
  expect(summary(runHoisted(['remove', '@pnpm.e2e/foo'])))
    .toBe('dependencies:\n- @pnpm.e2e/foo 100.1.0')
})

test('the hoisted linker reports both sides of a version change', async () => {
  prepare()
  runHoisted(['add', '@pnpm.e2e/foo@100.0.0'])

  expect(summary(runHoisted(['add', '@pnpm.e2e/foo@100.1.0'])))
    .toBe('dependencies:\n- @pnpm.e2e/foo 100.0.0\n+ @pnpm.e2e/foo 100.1.0')
})

test('the hoisted linker reports the dependencies it restores, and only those', async () => {
  prepare()
  runHoisted(['add', '@pnpm.e2e/foo@100.0.0'])

  expect(summary(runHoisted(['install']))).toBeUndefined()

  fs.rmSync('node_modules', { recursive: true, force: true })
  expect(summary(runHoisted(['install'])))
    .toBe('dependencies:\n+ @pnpm.e2e/foo 100.0.0')
})

test('the hoisted linker reports an aliased dependency under its alias', async () => {
  prepare()

  // The alias is the directory under `node_modules`; the package behind it
  // has its own name, and the summary names both.
  expect(summary(runHoisted(['add', 'aliased@npm:@pnpm.e2e/foo@100.1.0'])))
    .toBe('dependencies:\n+ aliased <- @pnpm.e2e/foo 100.1.0')
})
