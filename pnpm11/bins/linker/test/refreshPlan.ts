import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'

import { expect, jest, test } from '@jest/globals'
import { readModulesDir } from '@pnpm/fs.read-modules-dir'
import { temporaryDirectory } from 'tempy'

const readDirectory = jest.fn(readModulesDir)
jest.unstable_mockModule('@pnpm/fs.read-modules-dir', () => ({ readModulesDir: readDirectory }))
const { createBinRefreshPlan, linkBins } = await import('../src/index.js')

test('refresh plans scan a nested directory once and refresh scoped commands on every platform', async () => {
  const modules = path.join(temporaryDirectory(), 'node_modules/parent/node_modules')
  const pkgRoot = path.join(modules, '@scope/tool')
  const bins = path.join(modules, '.bin')
  fs.mkdirSync(pkgRoot, { recursive: true })
  fs.writeFileSync(path.join(pkgRoot, 'package.json'), JSON.stringify({ name: '@scope/tool', version: '1.0.0', bin: { tool: 'cli.js' } }))
  fs.writeFileSync(path.join(pkgRoot, 'cli.js'), '#!/usr/bin/env node\nconsole.log("placeholder")\n', { mode: 0o755 })
  await linkBins(modules, bins, { warn: () => {} })
  readDirectory.mockClear()
  const refresh = await createBinRefreshPlan(modules, bins, { warn: () => {} })
  fs.writeFileSync(path.join(pkgRoot, 'cli.js'), '#!/usr/bin/env node --no-warnings\nconsole.log(process.execArgv.includes("--no-warnings") ? "refreshed" : "stale")\n')
  await refresh(new Set([pkgRoot]))
  expect(runCommand(path.join(bins, 'tool'))).toBe('refreshed\n')

  fs.writeFileSync(path.join(pkgRoot, 'package.json'), JSON.stringify({ name: '@scope/tool', version: '1.0.0', bin: { tool: 'replacement.js' } }))
  fs.writeFileSync(path.join(pkgRoot, 'replacement.js'), '#!/usr/bin/env node\nconsole.log("replacement")\n', { mode: 0o755 })
  await refresh(new Set([pkgRoot]))
  expect(runCommand(path.join(bins, 'tool'))).toBe('replacement\n')
  fs.writeFileSync(path.join(pkgRoot, 'package.json'), JSON.stringify({ name: '@scope/tool', version: '1.0.0', bin: {} }))
  await refresh(new Set([pkgRoot]))
  expect(fs.existsSync(path.join(bins, process.platform === 'win32' ? 'tool.cmd' : 'tool'))).toBe(false)
  expect(readDirectory).toHaveBeenCalledTimes(1)
})

test('removing a command preserves another command named with its Windows suffix', async () => {
  const modules = path.join(temporaryDirectory(), 'node_modules')
  const bins = path.join(modules, '.bin')
  for (const name of ['tool', 'tool.cmd']) {
    const pkgRoot = path.join(modules, name)
    fs.mkdirSync(pkgRoot, { recursive: true })
    fs.writeFileSync(path.join(pkgRoot, 'package.json'), JSON.stringify({ name, version: '1.0.0', bin: { [name]: 'cli.js' } }))
    fs.writeFileSync(path.join(pkgRoot, 'cli.js'), '#!/usr/bin/env node\nconsole.log("retained")\n', { mode: 0o755 })
  }
  await linkBins(modules, bins, { warn: () => {} })
  const refresh = await createBinRefreshPlan(modules, bins, { warn: () => {} })
  const removedRoot = path.join(modules, 'tool')
  fs.writeFileSync(path.join(removedRoot, 'package.json'), JSON.stringify({ name: 'tool', version: '1.0.0', bin: {} }))
  await refresh(new Set([removedRoot]))
  expect(fs.existsSync(path.join(bins, 'tool.cmd'))).toBe(true)
  expect(runCommand(path.join(bins, 'tool.cmd'))).toBe('retained\n')
})

test('refreshing a winning alias preserves discovery order when package names and versions tie', async () => {
  const modules = path.join(temporaryDirectory(), 'node_modules')
  const bins = path.join(modules, '.bin')
  const aliases = ['first-alias', 'second-alias']
  for (const alias of aliases) {
    const pkgRoot = path.join(modules, alias)
    fs.mkdirSync(pkgRoot, { recursive: true })
    fs.writeFileSync(path.join(pkgRoot, 'package.json'), JSON.stringify({ name: 'tool', version: '1.0.0', bin: { tool: 'cli.js' } }))
    fs.writeFileSync(path.join(pkgRoot, 'cli.js'), `#!/usr/bin/env node\nconsole.log(${JSON.stringify(alias)})\n`, { mode: 0o755 })
  }
  readDirectory.mockResolvedValueOnce(aliases)
  await linkBins(modules, bins, { warn: () => {} })
  expect(runCommand(path.join(bins, 'tool'))).toBe('first-alias\n')
  readDirectory.mockResolvedValueOnce(aliases)
  const refresh = await createBinRefreshPlan(modules, bins, { warn: () => {} })
  await refresh(new Set([path.join(modules, 'first-alias')]))
  expect(runCommand(path.join(bins, 'tool'))).toBe('first-alias\n')
  await refresh(new Set([path.join(modules, 'second-alias')]))
  expect(runCommand(path.join(bins, 'tool'))).toBe('first-alias\n')
})

function runCommand (bin: string): string {
  const result = process.platform === 'win32'
    ? spawnSync(`"${bin}.cmd"`, { shell: process.env.ComSpec ?? 'cmd.exe', encoding: 'utf8' })
    : spawnSync(bin, { encoding: 'utf8' })
  expect(result.stderr).toBe('')
  expect(result.status).toBe(0)
  return result.stdout.replaceAll('\r\n', '\n')
}
