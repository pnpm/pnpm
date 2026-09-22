/// <reference path="../../../__typings__/index.d.ts"/>
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { linkBins } from '@pnpm/bins.linker'
import isWindows from 'is-windows'
import { temporaryDirectory } from 'tempy'

const testOnPosix = isWindows() ? test.skip : test

testOnPosix.each([0o555, 0o755])('linking an executable bin with mode %i leaves its permissions and ctime unchanged', async (mode) => {
  const project = temporaryDirectory()
  const modules = path.join(project, 'node_modules')
  const pkg = path.join(modules, 'tool')
  fs.mkdirSync(pkg, { recursive: true })
  fs.writeFileSync(path.join(pkg, 'package.json'), JSON.stringify({ name: 'tool', bin: 'cli.js' }))
  const source = path.join(pkg, 'cli.js')
  fs.writeFileSync(source, '#!/usr/bin/env node\nconsole.log("ok")\n')
  fs.chmodSync(source, mode)
  const before = fs.statSync(source, { bigint: true })

  await linkBins(modules, path.join(modules, '.bin'), { warn: () => {} })

  const after = fs.statSync(source, { bigint: true })
  expect(after.mode).toBe(before.mode)
  expect(after.ctimeNs).toBe(before.ctimeNs)
})

testOnPosix.each([false, true])('linking a workspace bin preserves its source mode (preferSymlinkedExecutables=%s)', async (preferSymlinkedExecutables) => {
  const project = temporaryDirectory()
  const modules = path.join(project, 'node_modules')
  const pkg = path.join(project, 'packages', 'tool')
  fs.mkdirSync(pkg, { recursive: true })
  fs.mkdirSync(modules)
  fs.writeFileSync(path.join(pkg, 'package.json'), JSON.stringify({ name: 'tool', bin: 'cli.js' }))
  const source = path.join(pkg, 'cli.js')
  fs.writeFileSync(source, '#!/usr/bin/env node\nconsole.log("ok")\n')
  fs.chmodSync(source, 0o644)
  fs.symlinkSync(pkg, path.join(modules, 'tool'))
  const binDir = path.join(modules, '.bin')

  await linkBins(modules, binDir, { warn: () => {}, preferSymlinkedExecutables })
  expect(fs.statSync(source).mode & 0o777).toBe(0o644)
  await linkBins(modules, binDir, { warn: () => {}, preferSymlinkedExecutables })
  expect(fs.statSync(source).mode & 0o777).toBe(0o644)
  if (!preferSymlinkedExecutables) {
    const result = spawnSync(path.join(binDir, 'tool'), { encoding: 'utf8' })
    expect(result.status).toBe(0)
    expect(result.stdout.trim()).toBe('ok')
  }
})

testOnPosix.each(['cli.js', 'node_modules'])('linking a bin symlink preserves the external source %s', async (filename) => {
  const project = temporaryDirectory()
  const modules = path.join(project, 'deps', 'node_modules')
  const pkg = path.join(modules, 'tool')
  fs.mkdirSync(pkg, { recursive: true })
  fs.writeFileSync(path.join(pkg, 'package.json'), JSON.stringify({ name: 'tool', bin: 'cli.js' }))
  const source = path.join(project, filename)
  fs.writeFileSync(source, '#!/usr/bin/env node\nconsole.log("ok")\n')
  fs.chmodSync(source, 0o644)
  fs.symlinkSync(source, path.join(pkg, 'cli.js'))
  const binDir = path.join(modules, '.bin')

  await linkBins(modules, binDir, { warn: () => {} })

  expect(fs.statSync(source).mode & 0o777).toBe(0o644)
  expect(fs.existsSync(path.join(binDir, 'tool'))).toBe(true)
})
