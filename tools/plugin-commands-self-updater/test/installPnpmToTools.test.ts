import fs from 'fs'
import path from 'path'

import { tempDir } from '@pnpm/prepare'
import {
  exePlatformPkgDirNames,
  linkExePlatformBinary,
  pnpmPackageNameToInstall,
} from '@pnpm/tools.plugin-commands-self-updater'

const NATIVE_BYTES = '#!/native/pnpm binary bytes'
const PLACEHOLDER = 'This is a placeholder. Reinstall with build scripts enabled.'
const EXECUTABLE = process.platform === 'win32' ? 'pnpm.exe' : 'pnpm'
const V12_BIN_NAMES = ['pnpm', 'pn', 'pnpx', 'pnx']

describe('pnpmPackageNameToInstall', () => {
  test('always converges on the native `pnpm` package for v12+, regardless of the running package', () => {
    expect(pnpmPackageNameToInstall('12.0.0', 'pnpm')).toBe('pnpm')
    expect(pnpmPackageNameToInstall('12.0.0', '@pnpm/exe')).toBe('pnpm')
    expect(pnpmPackageNameToInstall('12.3.4-alpha.2', '@pnpm/exe')).toBe('pnpm')
    expect(pnpmPackageNameToInstall('13.0.0', '@pnpm/exe')).toBe('pnpm')
  })

  test('keeps the running package name for majors below 12', () => {
    expect(pnpmPackageNameToInstall('9.1.0', 'pnpm')).toBe('pnpm')
    expect(pnpmPackageNameToInstall('10.5.0', '@pnpm/exe')).toBe('@pnpm/exe')
    expect(pnpmPackageNameToInstall('11.0.0', 'pnpm')).toBe('pnpm')
  })
})

describe('exePlatformPkgDirNames', () => {
  test('lists the legacy name first, then the v12 `exe.<platform>-<arch>` name', () => {
    expect(exePlatformPkgDirNames('darwin', 'arm64')).toStrictEqual(['macos-arm64', 'exe.darwin-arm64'])
    expect(exePlatformPkgDirNames('darwin', 'x64')).toStrictEqual(['macos-x64', 'exe.darwin-x64'])
    expect(exePlatformPkgDirNames('win32', 'x64')).toStrictEqual(['win-x64', 'exe.win32-x64'])
  })

  test('includes both glibc and musl candidates on linux', () => {
    expect(exePlatformPkgDirNames('linux', 'x64')).toStrictEqual([
      'linux-x64',
      'linuxstatic-x64',
      'exe.linux-x64',
      'exe.linux-x64-musl',
    ])
  })

  test('normalizes the legacy Windows ia32 arch to x86', () => {
    expect(exePlatformPkgDirNames('win32', 'ia32')).toStrictEqual(['win-x86', 'exe.win32-ia32'])
  })
})

describe('linkExePlatformBinary', () => {
  test('relinks the pnpm v12 wrapper placeholder to the native binary', () => {
    const nativeDir = exePlatformPkgDirNames(process.platform, process.arch).find((n) => n.startsWith('exe.'))!
    const stage = setupStage({ wrapperPkgName: 'pnpm', nativeDir, binNames: V12_BIN_NAMES })

    linkExePlatformBinary(stage, 'pnpm')

    assertLinked(stage, 'pnpm')
  })

  test('relinks a legacy @pnpm/exe wrapper placeholder to the native binary', () => {
    const nativeDir = exePlatformPkgDirNames(process.platform, process.arch).find((n) => !n.startsWith('exe.'))!
    const stage = setupStage({ wrapperPkgName: '@pnpm/exe', nativeDir, binNames: ['pnpm'] })

    linkExePlatformBinary(stage, '@pnpm/exe')

    assertLinked(stage, '@pnpm/exe')
  })

  test('is a no-op (does not throw) when the wrapper package is not present', () => {
    const stage = tempDir(false)
    expect(() => {
      linkExePlatformBinary(stage, 'pnpm')
    }).not.toThrow()
  })

  test('is a no-op (does not throw) when no native platform package materialized', () => {
    const stage = setupStage({ wrapperPkgName: 'pnpm', nativeDir: undefined, binNames: V12_BIN_NAMES })

    expect(() => {
      linkExePlatformBinary(stage, 'pnpm')
    }).not.toThrow()

    // The placeholder is left untouched — there was nothing to link.
    const wrapperDir = path.join(stage, 'node_modules', 'pnpm')
    expect(fs.readFileSync(path.join(wrapperDir, 'pnpm'), 'utf8')).toBe(PLACEHOLDER)
  })
})

interface SetupStageOptions {
  wrapperPkgName: string
  nativeDir: string | undefined
  binNames: string[]
}

function setupStage ({ wrapperPkgName, nativeDir, binNames }: SetupStageOptions): string {
  const stage = tempDir(false)
  if (nativeDir != null) {
    const platformPkgDir = path.join(stage, 'node_modules', '@pnpm', nativeDir)
    fs.mkdirSync(platformPkgDir, { recursive: true })
    fs.writeFileSync(path.join(platformPkgDir, EXECUTABLE), NATIVE_BYTES)
  }
  const wrapperDir = path.join(stage, 'node_modules', ...wrapperPkgName.split('/'))
  fs.mkdirSync(wrapperDir, { recursive: true })
  const bin = Object.fromEntries(binNames.map((name) => [name, name]))
  fs.writeFileSync(path.join(wrapperDir, 'package.json'), JSON.stringify({ name: wrapperPkgName, bin }, null, 2))
  for (const name of binNames) {
    fs.writeFileSync(path.join(wrapperDir, name), PLACEHOLDER)
  }
  return stage
}

function assertLinked (stage: string, wrapperPkgName: string): void {
  const wrapperDir = path.join(stage, 'node_modules', ...wrapperPkgName.split('/'))
  if (process.platform === 'win32') {
    // On Windows every bin becomes a `.exe` and the extension-less twin, and the
    // manifest's bin field is rewritten to point at the `.exe` files.
    expect(fs.readFileSync(path.join(wrapperDir, 'pnpm.exe'), 'utf8')).toBe(NATIVE_BYTES)
    expect(fs.readFileSync(path.join(wrapperDir, 'pnpm'), 'utf8')).toBe(NATIVE_BYTES)
    const manifest = JSON.parse(fs.readFileSync(path.join(wrapperDir, 'package.json'), 'utf8'))
    expect(manifest.bin.pnpm).toBe('pnpm.exe')
  } else {
    // On Unix only the `pnpm` placeholder is relinked; pn/pnpx/pnx stay as the
    // committed shell scripts (when present).
    expect(fs.readFileSync(path.join(wrapperDir, 'pnpm'), 'utf8')).toBe(NATIVE_BYTES)
  }
}
