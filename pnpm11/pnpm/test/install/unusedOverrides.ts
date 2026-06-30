import fs from 'node:fs'

import { expect, test } from '@jest/globals'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpmSync } from '../utils/index.js'

test('warns when an override matches no dependency', async () => {
  prepareEmpty()

  fs.writeFileSync('package.json', JSON.stringify({
    name: 'root',
    private: true,
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  }))

  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['**', '!store/**'],
    overrides: {
      // @pnpm.e2e/dep-of-pkg-with-1-dep is a real transitive dep of
      // @pnpm.e2e/pkg-with-1-dep; the other two selectors match nothing.
      '@pnpm.e2e/dep-of-pkg-with-1-dep': '101.0.0',
      'this-overrides-key-matches-nothing': '1.0.0',
      '@pnpm.e2e/does-not-exist>@pnpm.e2e/bar': '1.0.0',
    },
  })

  const { stdout } = execPnpmSync(['install'], { expectSuccess: true })

  const output = stdout.toString()
  // Scope assertions to WARN lines so pnpm's progress output (which may
  // surface package names in resolved/fetched/imported counters) cannot
  // make the test pass or fail spuriously.
  const warnLines = output.split('\n').filter((line) => line.includes('[WARN]'))
  expect(warnLines.some((line) => /overrides? matched no dependency/.test(line))).toBe(true)
  expect(warnLines.some((line) => line.includes('this-overrides-key-matches-nothing'))).toBe(true)
  expect(warnLines.some((line) => line.includes('@pnpm.e2e/does-not-exist>@pnpm.e2e/bar'))).toBe(true)
  // The applied override must not appear in any unused-override warning line.
  expect(warnLines.some((line) => /@pnpm\.e2e\/dep-of-pkg-with-1-dep\b/.test(line))).toBe(false)
})

test('does not warn when every override matched', async () => {
  prepareEmpty()

  fs.writeFileSync('package.json', JSON.stringify({
    name: 'root',
    private: true,
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  }))

  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['**', '!store/**'],
    overrides: {
      '@pnpm.e2e/dep-of-pkg-with-1-dep': '101.0.0',
    },
  })

  const { stdout } = execPnpmSync(['install'], { expectSuccess: true })

  const warnLines = stdout.toString().split('\n').filter((line) => line.includes('[WARN]'))
  expect(warnLines.some((line) => /overrides? matched no dependency/.test(line))).toBe(false)
})

test('warns on unused per-project overrides when sharedWorkspaceLockfile is false', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: {
        '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',
    },
  ])

  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['**', '!store/**'],
    sharedWorkspaceLockfile: false,
    packageConfigs: {
      'project-1': {
        // @pnpm.e2e/dep-of-pkg-with-1-dep is a real transitive dep of
        // @pnpm.e2e/pkg-with-1-dep; the other selector matches nothing.
        overrides: {
          '@pnpm.e2e/dep-of-pkg-with-1-dep': '101.0.0',
          'this-per-project-key-matches-nothing': '1.0.0',
        },
      },
    },
  })

  const { stdout } = execPnpmSync(['install'], { expectSuccess: true })

  // Scope assertions to WARN lines so pnpm's progress output (which may
  // surface package names in resolved/fetched/imported counters) cannot
  // make the test pass or fail spuriously.
  const warnLines = stdout.toString().split('\n').filter((line) => line.includes('[WARN]'))
  expect(warnLines.some((line) => /overrides? matched no dependency/.test(line))).toBe(true)
  expect(warnLines.some((line) => line.includes('this-per-project-key-matches-nothing'))).toBe(true)
  // The applied override must not appear in any unused-override warning line.
  expect(warnLines.some((line) => /@pnpm\.e2e\/dep-of-pkg-with-1-dep\b/.test(line))).toBe(false)
})
