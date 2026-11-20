import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'

import { execPnpm, execPnpmSync } from './utils/index.js'

test('env rm --global removes pnpm-managed Node when the global bin directory is not in PATH', async () => {
  prepare()
  const pnpmHome = path.resolve('pnpm-home')
  const globalBin = path.join(pnpmHome, 'bin')
  const globalPkgDir = path.join(pnpmHome, 'global', 'v11')
  const installDir = path.join(globalPkgDir, 'install-node')
  const nodeDir = path.join(installDir, 'node_modules', 'node')
  fs.mkdirSync(path.join(nodeDir, 'bin'), { recursive: true })
  fs.writeFileSync(path.join(installDir, 'package.json'), JSON.stringify({ dependencies: { node: 'runtime:22.11.0' } }))
  fs.writeFileSync(path.join(nodeDir, 'package.json'), JSON.stringify({ name: 'node', version: '22.11.0', bin: { node: 'bin/node' } }))
  fs.writeFileSync(path.join(nodeDir, 'bin', 'node'), '')
  fs.symlinkSync(installDir, path.join(globalPkgDir, 'hash-node'), process.platform === 'win32' ? 'junction' : 'dir')
  fs.mkdirSync(globalBin, { recursive: true })
  fs.mkdirSync(path.join(pnpmHome, 'nodejs', '22.5.0'), { recursive: true })
  fs.mkdirSync(path.join(pnpmHome, 'nodejs', '20.0.0'), { recursive: true })
  const env = { PNPM_HOME: pnpmHome, XDG_DATA_HOME: path.resolve('data') }

  await execPnpm(['env', 'rm', '--global', '22'], { env })

  expect(fs.existsSync(path.join(globalPkgDir, 'hash-node'))).toBe(false)
  expect(fs.existsSync(installDir)).toBe(false)
  expect(fs.existsSync(path.join(pnpmHome, 'nodejs', '22.5.0'))).toBe(false)
  expect(fs.existsSync(path.join(pnpmHome, 'nodejs', '20.0.0'))).toBe(true)

  const { status, stderr } = execPnpmSync(['env', 'use', '--global', '22'], { env })
  expect(status).not.toBe(0)
  expect(stderr.toString()).toContain('is not in PATH')
})
