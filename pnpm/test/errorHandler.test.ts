import { prepare, preparePackages } from '@pnpm/prepare'
import writeYamlFile from 'write-yaml-file'
import { execPnpmSync } from './utils'

test('should print json format error when publish --json failed', async () => {
  prepare({
    name: 'test-publish-package-no-version',
    version: undefined,
  })

  const { status, stdout } = execPnpmSync(['publish', '--dry-run', '--json'])

  expect(status).toBe(1)
  const { error } = JSON.parse(stdout.toString())
  expect(error?.code).toBe('ERR_PNPM_PACKAGE_VERSION_NOT_FOUND')
  expect(error?.message).toBe('Package version is not defined in the package.json.')
})

test('should print json format error when add dependency on workspace root', async () => {
  preparePackages([
    {
      name: 'project-a',
      version: '1.0.0',
    },
    {
      name: 'project-b',
      version: '1.0.0',
    },
  ])
  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  const { status, stdout } = execPnpmSync(['add', 'nanoid', '-p'])

  expect(status).toBe(1)
  const { error } = JSON.parse(stdout.toString())
  expect(error?.code).toBe('ERR_PNPM_ADDING_TO_ROOT')
})
