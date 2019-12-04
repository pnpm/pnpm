import { add } from '@pnpm/plugin-commands-installation'
import { preparePackages } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import path = require('path')
import test = require('tape')

const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}`

test('installing with "workspace:" should work even if link-workspace-packages is off', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '2.0.0',
    },
  ])

  await add.handler(['project-2@workspace:*'], {
    bail: false,
    cliOptions: {},
    dir: path.resolve('project-1'),
    include: {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    },
    linkWorkspacePackages: false,
    lock: true,
    pnpmfile: 'pnpmfile.js',
    rawConfig: { registry: REGISTRY_URL },
    rawLocalConfig: { registry: REGISTRY_URL },
    saveWorkspaceProtocol: false,
    sort: true,
    workspaceConcurrency: 1,
    workspaceDir: process.cwd(),
  })

  const pkg = await import(path.resolve('project-1/package.json'))

  t.deepEqual(pkg && pkg.dependencies, { 'project-2': 'workspace:^2.0.0' })

  await projects['project-1'].has('project-2')

  t.end()
})
