import { expect, it } from '@jest/globals'
import { iterateHashedGraphNodes } from '@pnpm/deps.graph-hasher'
import type { AllowBuild, DepPath } from '@pnpm/types'

it('gates built dep paths through the allowBuild policy by depPath', () => {
  const registryDepPath = 'foo@1.0.0' as DepPath
  const directTarballDepPath = 'foo@https://example.com/foo.tgz' as DepPath
  const checkedDepPaths: DepPath[] = []
  const allowBuild: AllowBuild = (depPath) => {
    checkedDepPaths.push(depPath)
    return depPath === registryDepPath
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
        version: '1.0.0',
      },
      {
        depPath: directTarballDepPath,
        name: 'foo',
        version: '1.0.0',
      },
    ].values(),
    allowBuild
  ))

  expect(checkedDepPaths).toStrictEqual([registryDepPath, directTarballDepPath])
})
