import path from 'node:path'

import { expect, test } from '@jest/globals'
import { getGlobalPackageDetails, scanGlobalPackages } from '@pnpm/global.packages'
import { add, update } from '@pnpm/installing.commands'
import { prepare } from '@pnpm/prepare'
import { addDistTag, REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'

const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}`

// `--latest` resolves the `latest` dist-tag, which can point at an older
// release than the one installed — that is what rolled a self-updated pnpm back
// in pnpm/pnpm#14270. An update must never move a global package backwards.
test('global update --latest keeps a package that latest would downgrade', async () => {
  prepare()
  const options = globalOptions()

  await addDistTag({ package: '@pnpm.e2e/multi-version-a', version: '2.1.0', distTag: 'latest' })
  await add.handler(options as any, ['@pnpm.e2e/multi-version-a@2.1.0']) // eslint-disable-line @typescript-eslint/no-explicit-any
  await addDistTag({ package: '@pnpm.e2e/multi-version-a', version: '1.0.0', distTag: 'latest' })

  await update.handler({ ...options, latest: true } as any) // eslint-disable-line @typescript-eslint/no-explicit-any

  const groups = scanGlobalPackages(path.resolve('global'))
  const installed = (await Promise.all(groups.map(getGlobalPackageDetails))).flat()
  expect(installed).toContainEqual(
    expect.objectContaining({ alias: '@pnpm.e2e/multi-version-a', version: '2.1.0' })
  )
})

function globalOptions (): Record<string, unknown> {
  return {
    allowBuilds: {},
    argv: { original: [] },
    bail: false,
    bin: path.resolve('bin'),
    cacheDir: path.resolve('cache'),
    cliOptions: {},
    deployAllFiles: false,
    dir: process.cwd(),
    excludeLinksFromLockfile: false,
    extraEnv: {},
    global: true,
    globalPkgDir: path.resolve('global'),
    include: { dependencies: true, devDependencies: true, optionalDependencies: true },
    lock: true,
    pnpmfile: ['.pnpmfile.cjs'],
    pnpmHomeDir: '',
    preferWorkspacePackages: true,
    configByUri: {},
    registriesByScope: { default: REGISTRY_URL },
    rootProjectManifestDir: '',
    sort: true,
    storeDir: path.resolve('pnpm-store'),
    userConfig: {},
    virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
    workspaceConcurrency: 1,
  }
}
