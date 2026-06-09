import { expect, it } from '@jest/globals'
import { iterateHashedGraphNodes } from '@pnpm/deps.graph-hasher'
import type { LockfileResolution, PackageSnapshot } from '@pnpm/lockfile.types'
import type { AllowBuild, AllowBuildContext, DepPath } from '@pnpm/types'

it('passes trusted build identity context for registry tarball metadata', () => {
  const registryDepPath = 'foo@1.0.0' as DepPath
  const directTarballDepPath = 'foo@https://example.com/foo.tgz' as DepPath
  const registryResolution: LockfileResolution = {
    integrity: 'sha512-abc',
    tarball: 'https://registry.npmjs.org/foo/-/foo-1.0.0.tgz',
  }
  const directTarballResolution: LockfileResolution = {
    integrity: 'sha512-def',
    tarball: 'https://example.com/foo.tgz',
  }
  const checkedDepPaths: DepPath[] = []
  const contexts: Array<AllowBuildContext | undefined> = []
  const allowBuild: AllowBuild = (depPath, context) => {
    checkedDepPaths.push(depPath)
    contexts.push(context)
    return true
  }

  Array.from(iterateHashedGraphNodes(
    {
      [registryDepPath]: {
        children: {},
        fullPkgId: 'foo@1.0.0:sha512-abc',
      },
      [directTarballDepPath]: {
        children: {},
        fullPkgId: 'foo@1.0.0:sha512-def',
      },
    },
    [
      {
        depPath: registryDepPath,
        name: 'foo',
        pkgSnapshot: { resolution: registryResolution } as PackageSnapshot,
        version: '1.0.0',
      },
      {
        depPath: directTarballDepPath,
        name: 'foo',
        pkgSnapshot: { resolution: directTarballResolution } as PackageSnapshot,
        version: '1.0.0',
      },
    ].values(),
    allowBuild
  ))

  expect(contexts.map((context) => context?.trustPackageIdentity)).toStrictEqual([true, false])
  expect(checkedDepPaths).toStrictEqual([registryDepPath, directTarballDepPath])
})
