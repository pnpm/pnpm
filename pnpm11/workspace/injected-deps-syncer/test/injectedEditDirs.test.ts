import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepareEmpty } from '@pnpm/prepare'

import { injectedEditDirs, watchInjectedEdits } from '../src/index.js'

function writeJson (file: string, value: unknown): void {
  fs.mkdirSync(path.dirname(file), { recursive: true })
  fs.writeFileSync(file, JSON.stringify(value))
}

// Without a shared lockfile, each project's own `.modules.yaml` lists the
// copies injected into it, under the modules directory that project installs
// into. A directory the workspace patterns leave out is not a project.
test('injectedEditDirs reads the copies each project with its own lockfile lists', async () => {
  prepareEmpty()
  const workspaceDir = process.cwd()
  writeJson(path.join(workspaceDir, 'lib/package.json'), { name: 'lib', version: '1.0.0' })
  writeJson(path.join(workspaceDir, 'app/package.json'), { name: 'app', version: '1.0.0' })
  writeJson(path.join(workspaceDir, 'app/deps/.modules.yaml'), {
    injectedDeps: { '../lib': ['deps/.pnpm/lib@file+..+lib/node_modules/lib'] },
  })
  // An installed package under the custom modules directory is no project,
  // even though the broad pattern below matches it.
  writeJson(path.join(workspaceDir, 'app/deps/installed/package.json'), { name: 'installed', version: '1.0.0' })
  writeJson(path.join(workspaceDir, 'app/deps/installed/deps/.modules.yaml'), {
    injectedDeps: { '../../../lib': ['deps/.pnpm/lib@file+..+..+..+lib/node_modules/lib'] },
  })
  writeJson(path.join(workspaceDir, 'excluded/package.json'), { name: 'excluded', version: '1.0.0' })
  writeJson(path.join(workspaceDir, 'excluded/deps/.modules.yaml'), {
    injectedDeps: { '../lib': ['deps/.pnpm/lib@file+..+lib/node_modules/lib'] },
  })

  const sources = await injectedEditDirs({
    pkgName: 'lib',
    pkgRootDir: 'lib',
    workspaceDir,
    projectLockfiles: { workspacePackagePatterns: ['app/**', 'lib'], modulesDir: 'deps' },
  })

  expect(sources).toEqual([{
    sourceDir: path.join(workspaceDir, 'lib'),
    targetDirs: [path.join(workspaceDir, 'app/deps/.pnpm/lib@file+..+lib/node_modules/lib')],
  }])
})

test('injectedEditDirs reads the root\'s copies from its configured modules directory', async () => {
  prepareEmpty()
  const workspaceDir = process.cwd()
  writeJson(path.join(workspaceDir, 'package.json'), { name: 'root', version: '1.0.0' })
  writeJson(path.join(workspaceDir, 'lib/package.json'), { name: 'lib', version: '1.0.0' })
  writeJson(path.join(workspaceDir, 'deps/.modules.yaml'), {
    injectedDeps: { lib: ['deps/.pnpm/lib@file+lib/node_modules/lib'] },
  })

  const sources = await injectedEditDirs({
    pkgName: 'lib',
    pkgRootDir: 'lib',
    workspaceDir,
    projectLockfiles: { workspacePackagePatterns: ['.', 'lib'], modulesDir: 'deps' },
  })

  expect(sources).toEqual([{
    sourceDir: path.join(workspaceDir, 'lib'),
    targetDirs: [path.join(workspaceDir, 'deps/.pnpm/lib@file+lib/node_modules/lib')],
  }])
})

test('watchInjectedEdits logs a source directory it cannot inspect and stops cleanly', async () => {
  prepareEmpty()
  const loop = path.resolve('loop')
  fs.symlinkSync(loop, loop)
  const unhandled: unknown[] = []
  const onUnhandled = (reason: unknown): void => {
    unhandled.push(reason)
  }
  process.on('unhandledRejection', onUnhandled)
  try {
    const watch = watchInjectedEdits(loop, [path.resolve('copy')])
    await new Promise((resolve) => setTimeout(resolve, 500))
    await watch.stop()
    await new Promise((resolve) => setImmediate(resolve))
  } finally {
    process.off('unhandledRejection', onUnhandled)
  }
  expect(unhandled).toEqual([])
})
