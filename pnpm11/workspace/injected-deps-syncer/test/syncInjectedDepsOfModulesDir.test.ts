import fs from 'node:fs'
import path from 'node:path'

import { afterEach, expect, test } from '@jest/globals'
import { prepareEmpty } from '@pnpm/prepare'
import isWindows from 'is-windows'

import { syncInjectedDepsOfModulesDir } from '../src/index.js'

// Permission bits cannot make a directory unreadable on Windows or for root.
const testWithPermissions = isWindows() || process.getuid?.() === 0 ? test.skip : test

let lockedDir: string | undefined

afterEach(() => {
  if (lockedDir != null) {
    fs.chmodSync(lockedDir, 0o755)
    lockedDir = undefined
  }
})

function prepareInjectedCopy (): { lockfileDir: string, modulesDir: string, sourceDir: string, copy: string } {
  prepareEmpty()
  const lockfileDir = process.cwd()
  const modulesDir = path.join(lockfileDir, 'node_modules')
  const copy = path.join(lockfileDir, 'node_modules/.pnpm/shared@file+shared+dist/node_modules/shared')
  fs.mkdirSync(copy, { recursive: true })
  fs.writeFileSync(path.join(copy, 'index.js'), 'built')
  fs.writeFileSync(path.join(modulesDir, '.modules.yaml'), JSON.stringify({
    injectedDeps: {
      'shared/dist': [path.relative(lockfileDir, copy)],
    },
  }))
  fs.mkdirSync(path.join(lockfileDir, 'shared'))
  return { lockfileDir, modulesDir, sourceDir: path.join(lockfileDir, 'shared/dist'), copy }
}

test('syncInjectedDepsOfModulesDir leaves the copies alone when the source directory does not exist', async () => {
  const { lockfileDir, modulesDir, sourceDir, copy } = prepareInjectedCopy()

  await syncInjectedDepsOfModulesDir({ lockfileDir, modulesDir, sourceDirs: new Set([sourceDir]) })

  expect(fs.readFileSync(path.join(copy, 'index.js'), 'utf8')).toBe('built')
})

testWithPermissions('syncInjectedDepsOfModulesDir rejects when the source directory cannot be inspected', async () => {
  const { lockfileDir, modulesDir, sourceDir, copy } = prepareInjectedCopy()
  lockedDir = path.dirname(sourceDir)
  fs.chmodSync(lockedDir, 0o000)

  await expect(syncInjectedDepsOfModulesDir({ lockfileDir, modulesDir, sourceDirs: new Set([sourceDir]) }))
    .rejects.toMatchObject({ code: 'EACCES' })
  expect(fs.readFileSync(path.join(copy, 'index.js'), 'utf8')).toBe('built')
})
