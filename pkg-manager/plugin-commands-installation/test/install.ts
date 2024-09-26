import fs from 'fs'
import delay from 'delay'
import path from 'path'
import { add, install } from '@pnpm/plugin-commands-installation'
import { prepare, prepareEmpty } from '@pnpm/prepare'
import { sync as rimraf } from '@zkochan/rimraf'
import { DEFAULT_OPTS } from './utils'

const describeOnLinuxOnly = process.platform === 'linux' ? describe : describe.skip

test('install fails if no package.json is found', async () => {
  prepareEmpty()

  await expect(install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })).rejects.toThrow(/No package\.json found/)
})

test('install does not fail when a new package is added', async () => {
  prepareEmpty()

  await add.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  }, ['is-positive@1.0.0'])

  const pkg = await import(path.resolve('package.json'))

  expect(pkg?.dependencies).toStrictEqual({ 'is-positive': '1.0.0' })
})

test('install with no store integrity validation', async () => {
  prepareEmpty()

  await add.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  }, ['is-positive@1.0.0'])

  // We should have a short delay before modifying the file in the store.
  // Otherwise pnpm will not consider it to be modified.
  await delay(200)
  const readmePath = path.join(DEFAULT_OPTS.storeDir, 'v3/files/9a/f6af85f55c111108eddf1d7ef7ef224b812e7c7bfabae41c79cf8bc9a910352536963809463e0af2799abacb975f22418a35a1d170055ef3fdc3b2a46ef1c5')
  fs.writeFileSync(readmePath, 'modified', 'utf8')

  rimraf('node_modules')

  await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    verifyStoreIntegrity: false,
  })

  expect(fs.readFileSync('node_modules/is-positive/readme.md', 'utf8')).toBe('modified')
})

// Covers https://github.com/pnpm/pnpm/issues/7362
describeOnLinuxOnly('filters optional dependencies based on libc', () => {
  test.each([
    ['glibc', '@pnpm.e2e+only-linux-x64-glibc@1.0.0', '@pnpm.e2e+only-linux-x64-musl@1.0.0'],
    ['musl', '@pnpm.e2e+only-linux-x64-musl@1.0.0', '@pnpm.e2e+only-linux-x64-glibc@1.0.0'],
  ])('%p → installs %p, does not install %p', async (libc, found, notFound) => {
    const rootProjectManifest = {
      dependencies: {
        '@pnpm.e2e/support-different-architectures': '1.0.0',
      },
      pnpm: {
        supportedArchitectures: {
          os: ['linux'],
          cpu: ['x64'],
          libc: [libc],
        },
      },
    }

    prepare(rootProjectManifest)

    await install.handler({
      ...DEFAULT_OPTS,
      rootProjectManifest,
      dir: process.cwd(),
    })

    const pkgDirs = fs.readdirSync(path.resolve('node_modules', '.pnpm'))
    expect(pkgDirs).toContain('@pnpm.e2e+support-different-architectures@1.0.0')
    expect(pkgDirs).toContain(found)
    expect(pkgDirs).not.toContain(notFound)
  })
})
