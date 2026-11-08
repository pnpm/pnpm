import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepareEmpty } from '@pnpm/prepare'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpmSync } from '../utils/index.js'

const CONFIG = ['--config.verify-deps-before-run=error'] as const

/**
 * A project `app` with a dependency on the local directory `lib`, which is not
 * a workspace project. No validation records the manifest of `lib`, so the
 * gate has to read it. Returns the directory the commands run in.
 */
function prepareLocalDirectoryDependency (inWorkspace: boolean): string {
  prepareEmpty()
  if (inWorkspace) {
    writeYamlFileSync('pnpm-workspace.yaml', { packages: ['app'] })
    fs.writeFileSync('package.json', JSON.stringify({ name: 'root', private: true }))
  }
  fs.mkdirSync('app')
  fs.writeFileSync('app/package.json', JSON.stringify({ name: 'app', version: '1.0.0', dependencies: { lib: 'file:../lib' } }))
  writeDirectoryManifest('lib', {})
  writeDirectoryManifest('other', {})
  return inWorkspace ? process.cwd() : path.resolve('app')
}

function writeDirectoryManifest (name: string, dependencies: Record<string, string>): void {
  fs.mkdirSync(name, { recursive: true })
  fs.writeFileSync(path.join(name, 'package.json'), JSON.stringify({ name, version: '1.0.0', dependencies }))
}

function gatedExec (cwd: string): ReturnType<typeof execPnpmSync> {
  const filter = cwd === process.cwd() ? ['--filter=app'] : []
  return execPnpmSync([...CONFIG, ...filter, 'exec', 'node', '-e', ''], { cwd })
}

test.each([
  ['a workspace', true],
  ['a standalone project', false],
])('a dependency added to a local directory dependency of %s fails the gate', (_name, inWorkspace) => {
  const cwd = prepareLocalDirectoryDependency(inWorkspace)
  execPnpmSync(['install'], { cwd, expectSuccess: true })
  expect(gatedExec(cwd).status).toBe(0)

  writeDirectoryManifest('lib', { other: 'file:../other' })

  const { status, stdout } = gatedExec(cwd)
  expect(status).not.toBe(0)
  expect(stdout.toString()).toContain('ERR_PNPM_VERIFY_DEPS_BEFORE_RUN')
})

test('a local directory dependency that a readPackage hook extends passes the gate', () => {
  const cwd = prepareLocalDirectoryDependency(true)
  fs.writeFileSync('.pnpmfile.cjs', `module.exports = { hooks: { readPackage (pkg) {
  if (pkg.name === 'lib') pkg.dependencies = { ...pkg.dependencies, other: 'file:../other' }
  return pkg
} } }
`)
  execPnpmSync(['install'], { cwd, expectSuccess: true })
  expect(gatedExec(cwd).status).toBe(0)
})
