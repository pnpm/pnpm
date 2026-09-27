import { expect, jest, test } from '@jest/globals'

import { DEFAULT_OPTS } from './utils/index.js'

// The config reader decides whether a pnpm-workspace.yaml stands behind the
// configured catalogs, and the deps installer owns the frozen-install
// catalogs check. This handoff is the only link between them, so it gets
// pinned on its own: dropping the mapping would leave every neighbor green
// while manifest-free frozen installs compare catalogs again
// (pnpm/pnpm#10551).
const installDepsModule = await import('../lib/installDeps.js')
const installDeps = jest.fn<typeof installDepsModule.installDeps>()
jest.unstable_mockModule('../lib/installDeps.js', () => ({
  ...installDepsModule,
  installDeps,
}))

const { install } = await import('@pnpm/installing.commands')

test('a manifest-free project skips the recorded-catalogs check', async () => {
  await install.handler({ ...DEFAULT_OPTS, dir: process.cwd(), workspaceManifestFound: false })

  expect(installDeps).toHaveBeenCalledWith(
    expect.objectContaining({ ignoreRecordedCatalogs: true }),
    []
  )
})

test('a workspace project keeps the recorded-catalogs check on', async () => {
  await install.handler({ ...DEFAULT_OPTS, dir: process.cwd(), workspaceManifestFound: true })

  expect(installDeps).toHaveBeenCalledWith(
    expect.objectContaining({ ignoreRecordedCatalogs: false }),
    []
  )
})

test('an unset workspaceManifestFound keeps the recorded-catalogs check on', async () => {
  await install.handler({ ...DEFAULT_OPTS, dir: process.cwd() })

  expect(installDeps).toHaveBeenCalledWith(
    expect.objectContaining({ ignoreRecordedCatalogs: false }),
    []
  )
})

test('a catalog slot the global config file leaves undefined does not keep the check on', async () => {
  await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    workspaceManifestFound: false,
    catalogs: { default: undefined },
  })

  expect(installDeps).toHaveBeenCalledWith(
    expect.objectContaining({ ignoreRecordedCatalogs: true }),
    []
  )
})

test('an empty catalog set from a source other than a workspace manifest keeps the check on', async () => {
  await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    workspaceManifestFound: false,
    catalogs: {},
  })

  expect(installDeps).toHaveBeenCalledWith(
    expect.objectContaining({ ignoreRecordedCatalogs: false }),
    []
  )
})

test('catalogs from a source other than a workspace manifest keep the check on', async () => {
  // A pnpmfile `updateConfig` hook can supply catalogs to a project with no
  // pnpm-workspace.yaml. They must reach the pnpr server to resolve
  // `catalog:` specifiers, so they keep the check on too.
  await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    workspaceManifestFound: false,
    catalogs: { default: { 'is-positive': '^1.0.0' } },
  })

  expect(installDeps).toHaveBeenCalledWith(
    expect.objectContaining({ ignoreRecordedCatalogs: false }),
    []
  )
})
