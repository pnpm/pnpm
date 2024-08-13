import path from 'path'
import { getEditDirPath } from '../src/getEditDirPath'

test('getEditDirPath() returns path to pkg@version inside node_modules/.pnpm_patches', () => {
  expect(getEditDirPath('pkg', {
    alias: 'pkg',
    pref: '0.1.2',
  }, { modulesDir: 'node_modules' })).toBe(path.join('node_modules', '.pnpm_patches', 'pkg@0.1.2'))
})

test('getEditDirPath() returns path to pkg@version inside .pnpm_patches inside specified modules dir', () => {
  expect(getEditDirPath('pkg', {
    alias: 'pkg',
    pref: '0.1.2',
  }, {
    modulesDir: 'user-defined-modules-dir',
  })).toBe(path.join('user-defined-modules-dir', '.pnpm_patches', 'pkg@0.1.2'))
})

test('getEditDirPath() returns valid path even if pref contains special characters', () => {
  expect(getEditDirPath('pkg', {
    alias: 'pkg',
    pref: 'https://codeload.github.com/zkochan/hi/tar.gz',
  }, { modulesDir: 'node_modules' })).toBe(path.join('node_modules', '.pnpm_patches', 'pkg@https+codeload.github.com+zkochan+hi+tar.gz'))
})

test('getEditDirPath() returns path with name of alias if pref is not available', () => {
  expect(getEditDirPath('pkg', {
    alias: 'resolved-pkg',
  }, { modulesDir: 'node_modules' })).toBe(path.join('node_modules', '.pnpm_patches', 'resolved-pkg'))
})

test('getEditDirPath() returns path with name of param if alias is not available', () => {
  expect(getEditDirPath('pkg', {
    pref: '0.1.2',
  }, { modulesDir: 'node_modules' })).toBe(path.join('node_modules', '.pnpm_patches', 'pkg'))
})
