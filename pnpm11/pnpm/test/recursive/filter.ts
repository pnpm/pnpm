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

// Regression test for https://github.com/pnpm/pnpm/issues/15587
test('pnpm --filter "<pkg>..." run follows dependencies declared through a workspace catalog entry', async () => {
  preparePackages([
    {
      location: 'math',
      package: {
        name: '@acme/math',
        scripts: {
          which: "node -e \"console.log('from-math')\"",
        },
      },
    },
    {
      location: 'app',
      package: {
        name: '@acme/app',
        dependencies: {
          '@acme/math': 'catalog:',
        },
        scripts: {
          which: "node -e \"console.log('from-app')\"",
        },
      },
    },
  ])
  fs.writeFileSync('pnpm-workspace.yaml', 'packages:\n  - "*"\ncatalog:\n  "@acme/math": "workspace:*"\n')

  const result = execPnpmSync(['--filter', '@acme/app...', 'run', 'which'])
  expect(result.status).toBe(0)
  const stdout = result.stdout.toString()
  expect(stdout).toContain('from-math')
  expect(stdout.indexOf('from-math')).toBeLessThan(stdout.indexOf('from-app'))
})

test('pnpm --recursive --filter "!./packages/**" --filter "a" run should re-include package (pnpm/pnpm#9354)', async () => {
  preparePackages([
    {
      location: '.',
      package: {
        name: 'root',
        version: '0.0.0',
        private: true,
        scripts: {
          which: 'node -e "console.log(\'root\')"',
        },
      },
    },
    {
      location: 'packages/a',
      package: {
        name: 'a',
        version: '1.0.0',
        scripts: {
          which: 'node -e "console.log(\'a\')"',
        },
      },
    },
    {
      location: 'packages/b',
      package: {
        name: 'b',
        version: '1.0.0',
        scripts: {
          which: 'node -e "console.log(\'b\')"',
        },
      },
    },
  ])

  fs.writeFileSync('pnpm-workspace.yaml', 'packages:\n  - "packages/*"\n')

  const result = execPnpmSync([
    '--stream',
    '--config.verify-deps-before-run=false',
    '--recursive',
    '--filter',
    '!./packages/**',
    '--filter',
    'a',
    'run',
    'which',
  ])
  expect(result.status).toBe(0)

  const stdout = result.stdout.toString()
  expect(stdout).toContain('packages/a which$')
  expect(stdout).not.toContain('packages/b which$')
})

test('pnpm --filter "<pkg>..." run follows an npm alias of a workspace project when workspace packages are linked', async () => {
  preparePackages([
    {
      location: 'math',
      package: {
        name: 'math',
        version: '1.0.0',
        scripts: {
          which: "node -e \"console.log('from-math')\"",
        },
      },
    },
    {
      location: 'app',
      package: {
        name: 'app',
        dependencies: {
          'math-alias': 'npm:math@^1.0.0',
        },
        scripts: {
          which: "node -e \"console.log('from-app')\"",
        },
      },
    },
  ])
  fs.writeFileSync('pnpm-workspace.yaml', 'packages:\n  - "*"\nlinkWorkspacePackages: true\n')

  const result = execPnpmSync(['--filter', 'app...', 'run', 'which'])
  expect(result.status).toBe(0)
  const stdout = result.stdout.toString()
  expect(stdout).toContain('from-math')
  expect(stdout.indexOf('from-math')).toBeLessThan(stdout.indexOf('from-app'))
})
