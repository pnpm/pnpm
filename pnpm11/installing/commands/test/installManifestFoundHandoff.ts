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
