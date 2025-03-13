import { preparePackages } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'
import { sync as writeYamlFile } from 'write-yaml-file'
import { execPnpmSync } from '../utils'

const f = fixtures(__dirname)

test('allowUnusedPatches=false errors on unused patches', async () => {
  preparePackages([
    {
      name: 'foo',
      version: '0.0.0',
      private: true,
    },
    {
      name: 'bar',
      version: '0.0.0',
      private: true,
    },
  ])

  const patchFile = f.find('patch-pkg/is-positive@1.0.0.patch')

  writeYamlFile('pnpm-workspace.yaml', {
    allowUnusedPatches: false,
    packages: ['**', '!store/**'],
    patchedDependencies: {
      'is-positive': patchFile,
    },
  })

  // pnpm install should fail
  const { status, stdout } = execPnpmSync(['install'])
  expect(status).not.toBe(0)
  expect(stdout.toString()).toContain('ERR_PNPM_UNUSED_PATCH')
  expect(stdout.toString()).toContain('The following patches were not used: is-positive')
})

test('allowUnusedPatches=true warns about unused patches', async () => {
  preparePackages([
    {
      name: 'foo',
      version: '0.0.0',
      private: true,
    },
    {
      name: 'bar',
      version: '0.0.0',
      private: true,
    },
  ])

  const patchFile = f.find('patch-pkg/is-positive@1.0.0.patch')

  writeYamlFile('pnpm-workspace.yaml', {
    allowUnusedPatches: true,
    packages: ['**', '!store/**'],
    patchedDependencies: {
      'is-positive': patchFile,
    },
  })

  // pnpm install should not fail
  const { stdout } = execPnpmSync(['install'], { expectSuccess: true })

  // pnpm install should print a warning regarding unused patches
  expect(stdout.toString()).toContain('The following patches were not used: is-positive')
})
