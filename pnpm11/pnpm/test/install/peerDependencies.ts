import fs from 'node:fs'

import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'

import { execPnpm } from '../utils/index.js'

test('auto-installed peer bins are linked at the workspace root after frozen reinstall', async () => {
  const project = prepare({
    devDependencies: { '@pnpm.e2e/pkg-with-peer-having-bin': '1.0.0' },
  })
  fs.mkdirSync('packages/app', { recursive: true })
  fs.writeFileSync('packages/app/package.json', '{"name":"app","version":"1.0.0"}')
  fs.writeFileSync('pnpm-workspace.yaml', 'packages:\n  - packages/*\nautoInstallPeers: true\n')

  await execPnpm(['install'])
  project.isExecutable('.bin/peer-with-bin')

  fs.rmSync('node_modules', { recursive: true })
  await execPnpm(['install', '--frozen-lockfile'])
  project.isExecutable('.bin/peer-with-bin')

  fs.rmSync('node_modules/.bin/peer-with-bin', { force: true })
  if (process.platform === 'win32') {
    fs.rmSync('node_modules/.bin/peer-with-bin.cmd', { force: true })
  }
  fs.writeFileSync('packages/app/package.json', JSON.stringify({
    name: 'app',
    version: '1.0.0',
    dependencies: { '@pnpm.e2e/hello-world-js-bin': '1.0.0' },
  }))
  await execPnpm(['install'])
  project.isExecutable('.bin/peer-with-bin')
})

test('transitive pending peer uses provider final suffix in lockfile', async () => {
  const project = prepare({
    dependencies: {
      '@pnpm.e2e/final-peer-a': '1.0.0',
      '@pnpm.e2e/final-peer-c': '1.0.0',
    },
  })

  await execPnpm(['install'])

  const lockfile = project.readLockfile()
  const snapshots = Object.keys(lockfile.snapshots)
  const expected = '@pnpm.e2e/final-peer-x@1.0.0(@pnpm.e2e/final-peer-b@1.0.0(@pnpm.e2e/final-peer-a@1.0.0(@pnpm.e2e/final-peer-c@1.0.0)))'
  const provisional = '@pnpm.e2e/final-peer-x@1.0.0(@pnpm.e2e/final-peer-b@1.0.0(@pnpm.e2e/final-peer-a@1.0.0))'

  expect(snapshots).toContain(expected)
  expect(snapshots).not.toContain(provisional)
})
