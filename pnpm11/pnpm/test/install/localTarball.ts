import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepareEmpty } from '@pnpm/prepare'

import {
  execPnpm,
  execPnpmSync,
} from '../utils/index.js'

test.each(['changed', 'deleted', 'directory', 'excluded', 'direct', 'fetch', 'unsupported-optional', 'lockfile-only', 'forced-unsupported-optional', 'fifo'])('frozen install verifies a %s local tarball from a warm store', async (mutation) => {
  if (mutation === 'fifo' && process.platform === 'win32') return
  prepareEmpty()
  const packageDir = path.resolve('local-tarball')
  const tarball = path.resolve('local-tarball.tgz')
  const isUnsupported = mutation === 'unsupported-optional' || mutation === 'forced-unsupported-optional'
  fs.mkdirSync(packageDir)
  fs.writeFileSync(path.join(packageDir, 'package.json'), JSON.stringify({
    name: 'local-tarball',
    version: '1.0.0',
    ...(isUnsupported ? { os: ['nonexistent-os'] } : {}),
  }))
  fs.writeFileSync(path.join(packageDir, 'index.js'), "module.exports = 'first'\n")
  packLocalTarball(packageDir, tarball)
  const parentDir = path.resolve('parent')
  fs.mkdirSync(parentDir)
  fs.writeFileSync(path.join(parentDir, 'package.json'), JSON.stringify({
    name: 'parent',
    version: '1.0.0',
    dependencies: { 'local-tarball': `file:${tarball}` },
  }))
  execPnpmSync(['pack', '--pack-destination', '..'], { cwd: parentDir, expectSuccess: true })
  fs.writeFileSync('package.json', JSON.stringify({
    name: 'project',
    version: '1.0.0',
    [mutation === 'excluded' ? 'devDependencies' : isUnsupported ? 'optionalDependencies' : 'dependencies']: mutation === 'direct' || isUnsupported
      ? { 'local-tarball': 'file:./local-tarball.tgz' }
      : { parent: 'file:./parent-1.0.0.tgz' },
  }))

  await execPnpm(['install'])

  if (['changed', 'direct', 'fetch'].includes(mutation)) {
    fs.writeFileSync(path.join(packageDir, 'index.js'), "module.exports = 'second'\n")
    packLocalTarball(packageDir, tarball)
  } else {
    fs.rmSync(tarball)
    if (mutation === 'directory') {
      fs.mkdirSync(tarball)
    } else if (mutation === 'fifo') {
      execFileSync('mkfifo', [tarball])
    }
  }

  if (mutation === 'fetch') {
    fs.writeFileSync('package.json', JSON.stringify({ name: 'project', version: '1.0.0' }))
    fs.writeFileSync('pnpm-lock.yaml', fs.readFileSync('pnpm-lock.yaml', 'utf8').replace('\n  .:', '\n  packages/app:'))
  }
  const installArgs = mutation === 'fetch'
    ? ['fetch']
    : mutation === 'lockfile-only'
      ? ['install', '--frozen-lockfile', '--lockfile-only']
      : mutation === 'forced-unsupported-optional'
        ? ['install', '--frozen-lockfile', '--force']
        : ['install', '--frozen-lockfile', ...(mutation === 'excluded' ? ['--prod'] : [])]
  const { status, stdout, stderr } = execPnpmSync(installArgs, { env: { CI: 'true' } })
  if (['excluded', 'unsupported-optional', 'lockfile-only'].includes(mutation)) {
    expect(status).toBe(0)
    return
  }
  expect(status).not.toBe(0)
  const output = stdout.toString() + stderr.toString()
  const expectedCode = ['changed', 'direct', 'fetch'].includes(mutation)
    ? 'ERR_PNPM_TARBALL_INTEGRITY'
    : 'ERR_PNPM_TARBALL_READ_LOCAL_TARBALL'
  expect(output).toContain(expectedCode)
  expect(output).toContain(tarball)
})

function packLocalTarball (packageDir: string, tarball: string): void {
  execPnpmSync(['pack', '--pack-destination', '..'], { cwd: packageDir, expectSuccess: true })
  fs.rmSync(tarball, { force: true })
  fs.renameSync(path.join(packageDir, '..', 'local-tarball-1.0.0.tgz'), tarball)
}
