import fs from 'node:fs'

import { expect, test } from '@jest/globals'
import { prepare, preparePackages } from '@pnpm/prepare'

import { execPnpmSync } from '../utils/index.js'

test('pnpm --filter <root> add <pkg> should work', async () => {
  prepare({
    name: 'root',
    version: '1.0.0',
  })

  fs.writeFileSync('pnpm-workspace.yaml', 'packages:\n  - "."\noverrides:\n  is-positive: "1.0.0"\n')

  const result = execPnpmSync(['--filter', 'root', 'add', 'is-positive'])
  if (result.status !== 0) {
    console.log(result.stdout.toString())
    console.log(result.stderr.toString())
  }
  expect(result.status).toBe(0)

  const pkg = JSON.parse(fs.readFileSync('package.json', 'utf8'))
  expect(pkg.dependencies['is-positive']).toBeTruthy()
})

test('pnpm --filter . add <pkg> should work', async () => {
  prepare({
    name: 'root',
    version: '1.0.0',
  })

  fs.writeFileSync('pnpm-workspace.yaml', 'packages:\n  - "."\n')

  const result = execPnpmSync(['--filter', '.', 'add', 'is-positive'])
  if (result.status !== 0) {
    console.log(result.stdout.toString())
    console.log(result.stderr.toString())
  }
  expect(result.status).toBe(0)

  const pkg = JSON.parse(fs.readFileSync('package.json', 'utf8'))
  expect(pkg.dependencies['is-positive']).toBeTruthy()
})

// Regression test for https://github.com/pnpm/pnpm/issues/11341
test('pnpm --recursive --filter "!<pkg>" run should still exclude the workspace root', async () => {
  preparePackages([
    {
      location: '.',
      package: {
        name: 'root',
        version: '0.0.0',
        private: true,
        scripts: {
          which: "node -e \"console.log('root')\"",
        },
      },
    },
    {
      location: 'a',
      package: {
        name: 'a',
        version: '1.0.0',
        scripts: {
          which: "node -e \"console.log('a')\"",
        },
      },
    },
    {
      location: 'b',
      package: {
        name: 'b',
        version: '1.0.0',
        scripts: {
          which: "node -e \"console.log('b')\"",
        },
      },
    },
  ])

  fs.writeFileSync('pnpm-workspace.yaml', 'packages:\n  - "*"\n')

  const result = execPnpmSync([
    '--stream',
    '--config.verify-deps-before-run=false',
    '--recursive',
    '--filter',
    '!a',
    'run',
    'which',
  ])
  expect(result.status).toBe(0)

  const stdout = result.stdout.toString()
  expect(stdout).toContain('b which$')
  // The `--stream` reporter prefixes lines with the project's relative directory,
  // so the workspace root (cwd === wsDir) would appear as `. which$` if included.
  expect(stdout).not.toContain('. which$')
  expect(stdout).not.toContain('a which$')
})

test('pnpm --recursive --filter "!<pkg>" --include-workspace-root run should include the workspace root', async () => {
  preparePackages([
    {
      location: '.',
      package: {
        name: 'root',
        version: '0.0.0',
        private: true,
        scripts: {
          which: "node -e \"console.log('root')\"",
        },
      },
    },
    {
      location: 'a',
      package: {
        name: 'a',
        version: '1.0.0',
        scripts: {
          which: "node -e \"console.log('a')\"",
        },
      },
    },
    {
      location: 'b',
      package: {
        name: 'b',
        version: '1.0.0',
        scripts: {
          which: "node -e \"console.log('b')\"",
        },
      },
    },
  ])

  fs.writeFileSync('pnpm-workspace.yaml', 'packages:\n  - "*"\n')

  const result = execPnpmSync([
    '--stream',
    '--config.verify-deps-before-run=false',
    '--recursive',
    '--include-workspace-root',
    '--filter',
    '!a',
    'run',
    'which',
  ])
  expect(result.status).toBe(0)

  const stdout = result.stdout.toString()
  expect(stdout).toContain('b which$')
  expect(stdout).toContain('. which$')
  expect(stdout).not.toContain('a which$')
})
