import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepareEmpty } from '@pnpm/prepare'

import { injectedEditDirs } from '../src/index.js'

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
  writeJson(path.join(workspaceDir, 'excluded/package.json'), { name: 'excluded', version: '1.0.0' })
  writeJson(path.join(workspaceDir, 'excluded/deps/.modules.yaml'), {
    injectedDeps: { '../lib': ['deps/.pnpm/lib@file+..+lib/node_modules/lib'] },
  })

  const sources = await injectedEditDirs({
    pkgName: 'lib',
    pkgRootDir: 'lib',
    workspaceDir,
    projectLockfiles: { workspacePackagePatterns: ['app', 'lib'], modulesDir: 'deps' },
  })

  expect(sources).toEqual([{
    sourceDir: path.join(workspaceDir, 'lib'),
    targetDirs: [path.join(workspaceDir, 'app/deps/.pnpm/lib@file+..+lib/node_modules/lib')],
  }])
})
