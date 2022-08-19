import { promises as fs } from 'fs'
import path from 'path'
import { preparePackages } from '@pnpm/prepare'
import writeYamlFile from 'write-yaml-file'
import { execPnpm } from '../utils'

test('pnpm recursive run finds bins from the root of the workspace', async () => {
  preparePackages([
    {
      location: '.',
      package: {
        dependencies: {
          'json-append': '1',
          '@pnpm.e2e/print-version': '2',
        },
      },
    },
    {
      name: 'project',
      version: '1.0.0',

      dependencies: {
        '@pnpm.e2e/print-version': '1',
      },
      scripts: {
        build: 'node -e "process.stdout.write(\'project-build\')" | json-append ../build-output.json',
        postinstall: 'node -e "process.stdout.write(\'project-postinstall\')" | json-append ../postinstall-output.json',
        testBinPriority: 'print-version | json-append ../testBinPriority.json',
      },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execPnpm(['-r', 'install'])

  expect(
    JSON.parse(await fs.readFile(path.resolve('postinstall-output.json'), 'utf8'))
  ).toStrictEqual(
    ['project-postinstall']
  )

  await execPnpm(['-r', 'run', 'build'])

  expect(
    JSON.parse(await fs.readFile(path.resolve('build-output.json'), 'utf8'))
  ).toStrictEqual(
    ['project-build']
  )

  process.chdir('project')
  await execPnpm(['run', 'build'])
  process.chdir('..')

  expect(
    JSON.parse(await fs.readFile(path.resolve('build-output.json'), 'utf8'))
  ).toStrictEqual(
    ['project-build', 'project-build']
  )

  await execPnpm(['recursive', 'rebuild'])

  expect(
    JSON.parse(await fs.readFile(path.resolve('postinstall-output.json'), 'utf8'))
  ).toStrictEqual(
    ['project-postinstall', 'project-postinstall']
  )

  await execPnpm(['recursive', 'run', 'testBinPriority'])

  expect(
    JSON.parse(await fs.readFile(path.resolve('testBinPriority.json'), 'utf8'))
  ).toStrictEqual(
    ['1.0.0\n']
  )
})
