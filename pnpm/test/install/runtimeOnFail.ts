import fs from 'node:fs'

import { prepare } from '@pnpm/prepare'

import { execPnpm } from '../utils/index.js'

test('runtimeOnFail=download causes Node.js to be downloaded even when the manifest does not set onFail', async () => {
  const project = prepare({
    devEngines: {
      runtime: {
        name: 'node',
        version: '24.0.0',
      },
    },
  })
  fs.writeFileSync('pnpm-workspace.yaml', 'runtimeOnFail: download\n', 'utf8')

  await execPnpm(['install'])

  project.isExecutable('.bin/node')
  const lockfile = project.readLockfile()
  expect(lockfile.importers['.'].devDependencies).toStrictEqual({
    node: {
      specifier: 'runtime:24.0.0',
      version: 'runtime:24.0.0',
    },
  })
})

test('runtimeOnFail=ignore prevents Node.js download even when manifest sets onFail=download', async () => {
  const project = prepare({
    devEngines: {
      runtime: {
        name: 'node',
        version: '24.0.0',
        onFail: 'download',
      },
    },
  })
  fs.writeFileSync('pnpm-workspace.yaml', 'runtimeOnFail: ignore\n', 'utf8')

  await execPnpm(['install'])

  const lockfile = project.readLockfile()
  expect(lockfile.importers['.'].devDependencies).toBeUndefined()
})
