import { preparePackages } from '@pnpm/prepare'
import { type LockfileObject } from '@pnpm/lockfile.types'
import { sync as writeYamlFile } from 'write-yaml-file'
import { sync as readYamlFile } from 'read-yaml-file'
import { execPnpm, execPnpmSync } from './utils'

test('ls --filter=not-exist --json should prints an empty array (#9672)', async () => {
  preparePackages([
    {
      location: 'packages/foo',
      package: {
        name: 'foo',
        version: '0.0.0',
        private: true,
      },
    },
  ])

  writeYamlFile('pnpm-workspace.yaml', {
    packages: ['packages/*'],
  })

  await execPnpm(['install'])
  expect(readYamlFile('pnpm-lock.yaml')).toStrictEqual(expect.objectContaining({
    importers: {
      'packages/foo': expect.any(Object),
    },
  } as Partial<LockfileObject>))

  const { stdout } = execPnpmSync(['ls', '--filter=project-that-does-not-exist', '--json'], { expectSuccess: true })
  expect(JSON.parse(stdout.toString())).toStrictEqual([])
})
