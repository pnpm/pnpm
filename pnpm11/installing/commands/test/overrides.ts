import { describe, expect, test } from '@jest/globals'
import { install, overrides } from '@pnpm/installing.commands'
import { prepare } from '@pnpm/prepare'

import { DEFAULT_OPTS } from './utils/index.js'

const baseOpts = (dir: string) => ({
  ...DEFAULT_OPTS,
  dir,
  lockfileDir: dir,
  workspaceDir: dir,
  recursive: true,
})

test('pnpm overrides check returns exit 0 with "No unused overrides" when every override matched', async () => {
  const project = prepare({
    name: 'root',
    version: '0.0.0',
    dependencies: { '@pnpm.e2e/pkg-with-1-dep': '100.0.0' },
  })

  // @pnpm.e2e/dep-of-pkg-with-1-dep is a real transitive dep of
  // @pnpm.e2e/pkg-with-1-dep — the override fires.
  const opts = {
    ...baseOpts(project.dir()),
    overrides: { '@pnpm.e2e/dep-of-pkg-with-1-dep': '101.0.0' },
  }
  // Seed: install is not strictly required (the command forces a full
  // re-resolution), but it warms the store and reproduces the realistic
  // invocation order.
  await install.handler(opts)

  const result = await overrides.handler(opts, [])
  expect(result).toEqual({ output: 'No unused overrides', exitCode: 0 })
})

test('pnpm overrides check returns exit 1 listing every unused selector', async () => {
  const project = prepare({
    name: 'root',
    version: '0.0.0',
    dependencies: { '@pnpm.e2e/pkg-with-1-dep': '100.0.0' },
  })

  const opts = {
    ...baseOpts(project.dir()),
    overrides: {
      '@pnpm.e2e/dep-of-pkg-with-1-dep': '101.0.0',
      'this-overrides-key-matches-nothing': '1.0.0',
      '@pnpm.e2e/does-not-exist>@pnpm.e2e/bar': '1.0.0',
    },
  }

  const result = await overrides.handler(opts, [])
  expect(result).toMatchObject({ exitCode: 1 })
  const output = (result as { output: string }).output
  // The applied override must not appear in the unused list.
  expect(output).not.toMatch(/@pnpm\.e2e\/dep-of-pkg-with-1-dep\b/)
  expect(output).toContain('this-overrides-key-matches-nothing')
  expect(output).toContain('@pnpm.e2e/does-not-exist>@pnpm.e2e/bar')
  // Selectors are sorted.
  expect(output.indexOf('this-overrides-key-matches-nothing'))
    .toBeGreaterThan(output.indexOf('@pnpm.e2e/does-not-exist>@pnpm.e2e/bar'))
})

test('pnpm overrides check --json returns JSON with unused array and exit 1', async () => {
  const project = prepare({
    name: 'root',
    version: '0.0.0',
    dependencies: { '@pnpm.e2e/pkg-with-1-dep': '100.0.0' },
  })

  const opts = {
    ...baseOpts(project.dir()),
    overrides: {
      '@pnpm.e2e/dep-of-pkg-with-1-dep': '101.0.0',
      'this-overrides-key-matches-nothing': '1.0.0',
      '@pnpm.e2e/does-not-exist>@pnpm.e2e/bar': '1.0.0',
    },
    json: true,
  }

  const result = await overrides.handler(opts, [])
  expect(result).toMatchObject({ exitCode: 1 })
  const parsed = JSON.parse((result as { output: string }).output)
  expect(parsed).toEqual({
    // Selectors are sorted.
    unused: [
      '@pnpm.e2e/does-not-exist>@pnpm.e2e/bar',
      'this-overrides-key-matches-nothing',
    ],
  })
})

test('pnpm overrides check --json returns { "unused": [] } with exit 0 when every override matched', async () => {
  const project = prepare({
    name: 'root',
    version: '0.0.0',
    dependencies: { '@pnpm.e2e/pkg-with-1-dep': '100.0.0' },
  })

  const opts = {
    ...baseOpts(project.dir()),
    overrides: { '@pnpm.e2e/dep-of-pkg-with-1-dep': '101.0.0' },
    json: true,
  }

  const result = await overrides.handler(opts, [])
  expect(result).toEqual({ output: JSON.stringify({ unused: [] }, null, 2), exitCode: 0 })
})

test('pnpm overrides check excludes convergence overrides from the unused diff', async () => {
  const project = prepare({
    name: 'root',
    version: '0.0.0',
    dependencies: { '@pnpm.e2e/pkg-with-1-dep': '100.0.0' },
  })

  // A convergence override selector ends with `@` and its value must be an
  // exact version. parseOverrides tags it with `converge: true`, which the
  // command filters out — convergence overrides have their own staleness path.
  // Combined with a normal selector that matches nothing, only the normal one
  // should appear in the unused list.
  const opts = {
    ...baseOpts(project.dir()),
    overrides: {
      '@pnpm.e2e/dep-of-pkg-with-1-dep@': '100.0.0',
      'this-overrides-key-matches-nothing': '1.0.0',
    },
  }

  const result = await overrides.handler(opts, [])
  expect(result).toMatchObject({ exitCode: 1 })
  const output = (result as { output: string }).output
  expect(output).toContain('this-overrides-key-matches-nothing')
  // The convergence selector must not be flagged as unused even though the
  // collector does not fire for convergence overrides.
  expect(output).not.toMatch(/@pnpm\.e2e\/dep-of-pkg-with-1-dep@/)
})

describe('pnpm overrides dispatch', () => {
  test('bare invocation runs check', async () => {
    const project = prepare({
      name: 'root',
      version: '0.0.0',
      dependencies: { '@pnpm.e2e/pkg-with-1-dep': '100.0.0' },
    })
    const opts = {
      ...baseOpts(project.dir()),
      overrides: { '@pnpm.e2e/dep-of-pkg-with-1-dep': '101.0.0' },
    }
    const bareResult = await overrides.handler(opts, [])
    const checkResult = await overrides.handler(opts, ['check'])
    expect(bareResult).toEqual(checkResult)
  })

  test('unknown subcommand returns help with exit 1', async () => {
    const project = prepare({
      name: 'root',
      version: '0.0.0',
      dependencies: { '@pnpm.e2e/pkg-with-1-dep': '100.0.0' },
    })
    const opts = {
      ...baseOpts(project.dir()),
      overrides: { '@pnpm.e2e/dep-of-pkg-with-1-dep': '101.0.0' },
    }
    const result = await overrides.handler(opts, ['unknown'])
    expect(result).toMatchObject({ exitCode: 1 })
    expect((result as { output: string }).output).toContain('pnpm overrides')
  })
})
