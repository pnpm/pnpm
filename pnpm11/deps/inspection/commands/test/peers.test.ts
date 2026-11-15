import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { peers } from '@pnpm/deps.inspection.commands'
import { tempDir } from '@pnpm/prepare'
import type { ProjectRootDir, ProjectsGraph } from '@pnpm/types'
import { writeYamlFileSync } from 'write-yaml-file'

function writePerProjectLockfile (projectDir: string, hasMissingPeer: boolean): void {
  fs.mkdirSync(projectDir, { recursive: true })
  fs.writeFileSync(path.join(projectDir, 'package.json'), JSON.stringify({
    name: path.basename(projectDir),
    version: '1.0.0',
  }))
  writeYamlFileSync(path.join(projectDir, 'pnpm-lock.yaml'), {
    lockfileVersion: '9.0',
    importers: {
      '.': hasMissingPeer
        ? {
          dependencies: {
            'peer-consumer': {
              specifier: '1.0.0',
              version: '1.0.0',
            },
          },
        }
        : {},
    },
    packages: hasMissingPeer
      ? {
        'peer-consumer@1.0.0': {
          resolution: { tarball: 'https://example.com/peer-consumer-1.0.0.tgz' },
          peerDependencies: {
            'missing-peer': '^1.0.0',
          },
        },
      }
      : {},
    snapshots: hasMissingPeer
      ? {
        'peer-consumer@1.0.0': {},
      }
      : {},
  })
}

test('peers checks per-project lockfiles when sharedWorkspaceLockfile is false', async () => {
  const workspaceDir = tempDir()
  const appDir = path.join(workspaceDir, 'packages/app') as ProjectRootDir
  const cleanDir = path.join(workspaceDir, 'packages/clean') as ProjectRootDir
  writePerProjectLockfile(appDir, true)
  writePerProjectLockfile(cleanDir, false)

  const selectedProjectsGraph = {
    [cleanDir]: {
      dependencies: [],
      package: {
        rootDir: cleanDir,
        rootDirRealPath: cleanDir,
        modulesDir: 'node_modules',
        manifest: { name: 'clean', version: '1.0.0' },
        writeProjectManifest: async () => {},
      },
    },
    [appDir]: {
      dependencies: [],
      package: {
        rootDir: appDir,
        rootDirRealPath: appDir,
        modulesDir: 'node_modules',
        manifest: { name: 'app', version: '1.0.0' },
        writeProjectManifest: async () => {},
      },
    },
  } as unknown as ProjectsGraph

  const recursiveText = await peers.handler({
    dir: workspaceDir,
    lockfileOnly: true,
    modulesDir: 'node_modules',
    recursive: true,
    selectedProjectsGraph,
    sharedWorkspaceLockfile: false,
    workspaceDir,
  }, ['check'])
  expect(recursiveText).toMatchObject({ exitCode: 1 })
  expect(typeof recursiveText === 'object' && recursiveText.output).toContain('missing-peer')

  // A stale root lockfile without a root project in selectedProjectsGraph is ignored.
  writeYamlFileSync(path.join(workspaceDir, 'pnpm-lock.yaml'), {
    lockfileVersion: '9.0',
    importers: { '.': {} },
  })
  const recursiveJson = await peers.handler({
    dir: workspaceDir,
    json: true,
    lockfileOnly: true,
    modulesDir: 'node_modules',
    recursive: true,
    selectedProjectsGraph,
    sharedWorkspaceLockfile: false,
    workspaceDir,
  }, [])
  expect(recursiveJson).toMatchObject({ exitCode: 1 })
  const parsed = JSON.parse(typeof recursiveJson === 'object' ? recursiveJson.output : recursiveJson)
  expect(Object.keys(parsed)).toEqual(['packages/app', 'packages/clean'])
  expect(parsed['packages/app'].missing['missing-peer']).toHaveLength(1)
  expect(parsed['packages/clean'].missing).toEqual({})

  // Single-project check inside packages/app uses '.' as the project key.
  const singleJson = await peers.handler({
    dir: appDir,
    json: true,
    lockfileOnly: true,
    modulesDir: 'node_modules',
    recursive: false,
    selectedProjectsGraph,
    sharedWorkspaceLockfile: false,
    workspaceDir,
  }, [])
  expect(singleJson).toMatchObject({ exitCode: 1 })
  const singleParsed = JSON.parse(typeof singleJson === 'object' ? singleJson.output : singleJson)
  expect(Object.keys(singleParsed)).toEqual(['.'])
  expect(singleParsed['.'].missing['missing-peer']).toHaveLength(1)

  const cleanResult = await peers.handler({
    dir: workspaceDir,
    lockfileOnly: true,
    modulesDir: 'node_modules',
    recursive: true,
    selectedProjectsGraph: { [cleanDir]: selectedProjectsGraph[cleanDir] } as unknown as ProjectsGraph,
    sharedWorkspaceLockfile: false,
    workspaceDir,
  }, [])
  expect(cleanResult).toEqual({
    output: 'No peer dependency issues found',
    exitCode: 0,
  })
})

test('peers honors packageConfigs modulesDir overrides and safely serializes __proto__ project keys', async () => {
  const workspaceDir = tempDir()
  const appDir = path.join(workspaceDir, 'packages/app') as ProjectRootDir
  const protoDir = path.join(workspaceDir, '__proto__') as ProjectRootDir
  writePerProjectLockfile(appDir, true)
  writePerProjectLockfile(protoDir, false)

  const customPnpmDir = path.join(appDir, 'custom_modules/.pnpm')
  fs.mkdirSync(customPnpmDir, { recursive: true })
  writeYamlFileSync(path.join(customPnpmDir, 'lock.yaml'), {
    lockfileVersion: '9.0',
    importers: { '.': {} },
  })

  const selectedProjectsGraph = {
    [appDir]: {
      dependencies: [],
      package: {
        rootDir: appDir,
        rootDirRealPath: appDir,
        manifest: { name: 'app', version: '1.0.0' },
        writeProjectManifest: async () => {},
      },
    },
    [protoDir]: {
      dependencies: [],
      package: {
        rootDir: protoDir,
        rootDirRealPath: protoDir,
        manifest: { name: 'proto-pkg', version: '1.0.0' },
        writeProjectManifest: async () => {},
      },
    },
  } as unknown as ProjectsGraph

  const currentResult = await peers.handler({
    dir: workspaceDir,
    json: true,
    modulesDir: 'node_modules',
    packageConfigs: {
      app: { modulesDir: 'custom_modules' },
    },
    recursive: true,
    selectedProjectsGraph,
    sharedWorkspaceLockfile: false,
    workspaceDir,
  }, [])
  expect(currentResult).toMatchObject({ exitCode: 0 })
  const currentOutput = typeof currentResult === 'object' ? currentResult.output : currentResult
  expect(currentOutput).toContain('"__proto__"')
  const parsedCurrent = JSON.parse(currentOutput) as Record<string, unknown>
  expect(Object.getPrototypeOf(parsedCurrent)).toBe(Object.prototype)

  const wantedResult = await peers.handler({
    dir: workspaceDir,
    lockfileOnly: true,
    modulesDir: 'node_modules',
    packageConfigs: {
      app: { modulesDir: 'custom_modules' },
    },
    recursive: true,
    selectedProjectsGraph,
    sharedWorkspaceLockfile: false,
    workspaceDir,
  }, [])
  expect(wantedResult).toMatchObject({ exitCode: 1 })
})
