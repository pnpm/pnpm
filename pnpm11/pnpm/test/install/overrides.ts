import fs from 'node:fs'

import { expect, test } from '@jest/globals'
import { prepareEmpty } from '@pnpm/prepare'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpmSync } from '../utils/index.js'

test('pnpm overrides exits 1 listing unused selectors', () => {
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

  // Seed: install gives a known-good baseline. The command forces its own
  // full re-resolution, but installing first matches the realistic flow.
  execPnpmSync(['install'], { expectSuccess: true })

  const result = execPnpmSync(['overrides'])
  expect(result.status).toBe(1)
  const output = result.stdout.toString()
  expect(output).toContain('unused override')
  // The applied override must not appear in the unused list.
  expect(output).not.toMatch(/@pnpm\.e2e\/dep-of-pkg-with-1-dep\b/)
  expect(output).toContain('this-overrides-key-matches-nothing')
  expect(output).toContain('@pnpm.e2e/does-not-exist>@pnpm.e2e/bar')
})

test('pnpm overrides exits 0 when every override matched', () => {
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

  execPnpmSync(['install'], { expectSuccess: true })

  const result = execPnpmSync(['overrides'])
  expect(result.status).toBe(0)
  expect(result.stdout.toString().trim()).toBe('No unused overrides')
})

test('pnpm overrides --json emits { "unused": [...] } with exit 1', () => {
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
      'this-overrides-key-matches-nothing': '1.0.0',
      '@pnpm.e2e/does-not-exist>@pnpm.e2e/bar': '1.0.0',
    },
  })

  execPnpmSync(['install'], { expectSuccess: true })

  const result = execPnpmSync(['overrides', '--json'])
  expect(result.status).toBe(1)
  // The CLI also emits install/progress output to stdout; isolate the JSON
  // payload by finding the first `{` and parsing from there.
  const stdout = result.stdout.toString()
  const jsonStart = stdout.indexOf('{')
  expect(jsonStart).toBeGreaterThanOrEqual(0)
  const parsed = JSON.parse(stdout.slice(jsonStart))
  expect(parsed).toEqual({
    // Selectors are sorted.
    unused: [
      '@pnpm.e2e/does-not-exist>@pnpm.e2e/bar',
      'this-overrides-key-matches-nothing',
    ],
  })
})

test('pnpm overrides --json emits { "unused": [] } with exit 0 when every override matched', () => {
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

  execPnpmSync(['install'], { expectSuccess: true })

  const result = execPnpmSync(['overrides', '--json'])
  expect(result.status).toBe(0)
  const stdout = result.stdout.toString()
  const jsonStart = stdout.indexOf('{')
  expect(jsonStart).toBeGreaterThanOrEqual(0)
  expect(JSON.parse(stdout.slice(jsonStart))).toEqual({ unused: [] })
})

test('pnpm overrides excludes convergence overrides from the unused diff', () => {
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
      // Convergence override: selector ends with `@`, value is exact version.
      // parseOverrides tags it converge: true; the command filters those out
      // because convergence overrides have their own staleness path and the
      // collector never fires for them.
      '@pnpm.e2e/dep-of-pkg-with-1-dep@': '100.0.0',
      'this-overrides-key-matches-nothing': '1.0.0',
    },
  })

  execPnpmSync(['install'], { expectSuccess: true })

  const result = execPnpmSync(['overrides'])
  expect(result.status).toBe(1)
  const output = result.stdout.toString()
  expect(output).toContain('this-overrides-key-matches-nothing')
  // The convergence selector must not be flagged as unused.
  expect(output).not.toMatch(/@pnpm\.e2e\/dep-of-pkg-with-1-dep@/)
})
