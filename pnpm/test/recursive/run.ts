import { preparePackages } from '@pnpm/prepare'
import { createTestIpcServer } from '@pnpm/test-ipc-server'
import { sync as writeYamlFile } from 'write-yaml-file'
import { execPnpm } from '../utils'

test('pnpm recursive run finds bins from the root of the workspace', async () => {
  await using serverForBuild = await createTestIpcServer()
  await using serverForPostInstall = await createTestIpcServer()
  await using serverForTestBinPriority = await createTestIpcServer()

  preparePackages([
    {
      location: '.',
      package: {
        dependencies: {
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
        build: serverForBuild.sendLineScript('project-build'),
        postinstall: serverForPostInstall.sendLineScript('project-postinstall'),
        testBinPriority: `print-version | ${serverForTestBinPriority.generateSendStdinScript()}`,
      },
    },
  ])

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execPnpm(['-r', 'install'])

  expect(serverForPostInstall.getLines()).toStrictEqual(['project-postinstall'])

  await execPnpm(['-r', 'run', 'build'])

  expect(serverForBuild.getLines()).toStrictEqual(['project-build'])

  process.chdir('project')
  await execPnpm(['run', 'build'])
  process.chdir('..')

  expect(serverForBuild.getLines()).toStrictEqual(['project-build', 'project-build'])

  await execPnpm(['recursive', 'rebuild'])

  expect(serverForPostInstall.getLines()).toStrictEqual(['project-postinstall', 'project-postinstall'])

  await execPnpm(['recursive', 'run', 'testBinPriority'])

  expect(serverForTestBinPriority.getLines()).toStrictEqual(['1.0.0'])
})
