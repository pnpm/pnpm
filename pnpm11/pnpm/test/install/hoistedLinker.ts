import fs from 'node:fs'

import { expect, test } from '@jest/globals'
import { prepare, preparePackages } from '@pnpm/prepare'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpmSync } from '../utils/index.js'

function runHoisted (args: string[]): string {
  return execPnpmSync([...args, '--config.node-linker=hoisted'], {
    env: { pnpm_config_silent: 'false' },
    stdio: 'pipe',
    expectSuccess: true,
  }).stdout.toString()
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

test('the hoisted linker does not report an optional dependency it skipped', async () => {
  prepare({
    optionalDependencies: {
      '@pnpm.e2e/not-compatible-with-any-os': '*',
    },
  })

  // The install resolves it and leaves it uninstalled, so the summary must
  // not claim otherwise.
  expect(summary(runHoisted(['install']))).toBeUndefined()
  expect(fs.existsSync('node_modules/@pnpm.e2e/not-compatible-with-any-os')).toBe(false)

  // The lockfile records what the last install resolved, so the entry is
  // still there for the next install that has work to do. Only the skip
  // that install recorded keeps it from reading as a package that has gone
  // away, which would put a removal line on every install from here on.
  const output = runHoisted(['add', '@pnpm.e2e/foo@100.0.0'])
  expect(summary(output)).toBe('dependencies:\n+ @pnpm.e2e/foo 100.0.0')
  expect(summary(output, 'optionalDependencies')).toBeUndefined()
})

test('the hoisted linker reports an optional dependency it stops supporting', async () => {
  const pkg = '@pnpm.e2e/not-compatible-with-any-os'
  prepare({
    optionalDependencies: {
      [pkg]: '*',
    },
  })

  writeYamlFileSync('pnpm-workspace.yaml', { supportedArchitectures: { os: ['this-os-does-not-exist'] } })
  expect(summary(runHoisted(['install']), 'optionalDependencies'))
    .toBe(`optionalDependencies:\n+ ${pkg} 1.0.0`)

  // Without the architecture it needs, the install skips the package and the
  // hoisted linker takes the directory away, so the summary has to say so.
  writeYamlFileSync('pnpm-workspace.yaml', {})
  expect(summary(runHoisted(['install']), 'optionalDependencies'))
    .toBe(`optionalDependencies:\n- ${pkg} 1.0.0`)
  expect(fs.existsSync(`node_modules/${pkg}`)).toBe(false)
})

/** The reporter's block for `group`, or `undefined` when it printed none. */
function summary (output: string, group: string = 'dependencies'): string | undefined {
  return new RegExp(`^${group}:\\n(?:[+-].*\\n?)+`, 'm').exec(output)?.[0].trim()
}

test('filter with node-linker=hoisted and shamefully-hoist=true only installs dependencies of filtered packages', async () => {
  preparePackages([
    {
      location: 'project-1',
      package: {
        name: 'project-1',
        version: '1.0.0',
        dependencies: {
          'is-positive': '1.0.0',
        },
        devDependencies: {
          'is-odd': '1.0.0',
        },
      },
    },
    {
      location: 'project-2',
      package: {
        name: 'project-2',
        version: '1.0.0',
        dependencies: {
          'is-negative': '1.0.0',
        },
      },
    },
  ])

  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['project-1', 'project-2'] })
  execPnpmSync([
    'install',
    '--filter',
    'project-1...',
    '--prod',
    '--config.node-linker=hoisted',
    '--config.shamefully-hoist=true',
  ], { expectSuccess: true })

  expect(fs.existsSync('node_modules/is-positive')).toBe(true)
  expect(fs.existsSync('node_modules/is-odd')).toBe(false)
  expect(fs.existsSync('node_modules/is-negative')).toBe(false)
})

