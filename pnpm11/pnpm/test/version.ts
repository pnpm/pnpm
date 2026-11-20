import { expect, test } from '@jest/globals'
import { prepare, preparePackages } from '@pnpm/prepare'
import { safeExeca as execa } from 'execa'
import { loadJsonFileSync } from 'load-json-file'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm, execPnpmSync, spawnPnpm, waitForPnpmExit } from './utils/index.js'

test('version --recursive bumps every workspace package', async () => {
  preparePackages([
    { name: 'project-1', version: '1.0.0' },
    { name: 'project-2', version: '2.3.0' },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['project-*'] })

  await execPnpm(['version', '--recursive', '--no-git-checks', 'minor'])

  expect(loadJsonFileSync<{ version: string }>('project-1/package.json').version).toBe('1.1.0')
  expect(loadJsonFileSync<{ version: string }>('project-2/package.json').version).toBe('2.4.0')
})

test('version --recursive --filter bumps only the selected package', async () => {
  preparePackages([
    { name: 'project-1', version: '1.0.0' },
    { name: 'project-2', version: '2.3.0' },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['project-*'] })

  await execPnpm(['version', '--recursive', '--filter', 'project-2', '--no-git-checks', 'patch'])

  expect(loadJsonFileSync<{ version: string }>('project-1/package.json').version).toBe('1.0.0')
  expect(loadJsonFileSync<{ version: string }>('project-2/package.json').version).toBe('2.3.1')
})

test('version --recursive --json prints an empty JSON array when there are no pending changes', async () => {
  preparePackages([
    { name: 'project-1', version: '1.0.0' },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['project-*'] })

  const result = execPnpmSync(['version', '--recursive', '--json', '--no-git-checks'])

  expect(result.status).toBe(0)
  expect(result.stdout.toString().trim()).toBe('[]')
})

test('version --recursive --json keeps stdout parseable, inherits stdin, and forwards script output to stderr', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      scripts: {
        preversion: 'node -e "console.log(\'preversion-stdin:\' + require(\'node:fs\').readFileSync(0, \'utf8\'))"',
        version: 'node -e "console.log(\'version-stdout\')"',
        postversion: 'node -e "console.log(\'postversion-stdout\')"',
      },
    },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['project-*'] })

  const proc = spawnPnpm(['version', '--recursive', 'patch', '--json', '--no-git-checks'])
  proc.stdin!.end('stdin-payload')
  const result = await waitForPnpmExit(proc)

  expect(result.status).toBe(0)
  expect(JSON.parse(result.stdout.toString().trim())).toEqual([
    expect.objectContaining({ name: 'project-1', currentVersion: '1.0.0', newVersion: '1.0.1' }),
  ])
  const stderr = result.stderr.toString()
  expect(stderr).toContain('preversion-stdin:stdin-payload')
  expect(stderr).toContain('version-stdout')
  expect(stderr).toContain('postversion-stdout')
})

test('version --recursive --json --loglevel=error suppresses passing script output and surfaces failing preversion output on stderr', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      scripts: {
        preversion: 'node -e "console.log(\'quiet-success-line\')"',
      },
    },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['project-*'] })

  const okResult = execPnpmSync(['version', '--recursive', 'patch', '--json', '--loglevel=error', '--no-git-checks'])
  expect(okResult.status).toBe(0)
  expect(okResult.stderr.toString()).not.toContain('quiet-success-line')

  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      scripts: {
        preversion: 'node -e "console.error(\'preversion-failure-detail\'); process.exit(1)"',
      },
    },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['project-*'] })

  const failResult = execPnpmSync(['version', '--recursive', 'patch', '--json', '--loglevel=error', '--no-git-checks'])

  expect(failResult.status).not.toBe(0)
  expect(failResult.stderr.toString()).toContain('preversion-failure-detail')
  expect(JSON.parse(failResult.stdout.toString().trim())).toEqual({
    error: expect.objectContaining({
      code: expect.any(String),
      message: expect.any(String),
    }),
  })
})

test('version from-git is wired through the compiled CLI', async () => {
  prepare({ name: 'test-pkg', version: '1.0.0' })

  await execa('git', ['init', '--initial-branch=main'])
  await execa('git', ['config', 'user.email', 'x@y.z'])
  await execa('git', ['config', 'user.name', 'xyz'])
  await execa('git', ['config', 'tag.gpgSign', 'false'])
  await execa('git', ['add', 'package.json'])
  await execa('git', ['commit', '-m', 'init', '--no-gpg-sign'])
  await execa('git', ['tag', 'v2.3.4'])

  await execPnpm(['version', 'from-git', '--no-git-checks', '--no-git-tag-version'])

  expect(loadJsonFileSync<{ version: string }>('package.json').version).toBe('2.3.4')
})
