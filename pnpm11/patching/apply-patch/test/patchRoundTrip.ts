import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'

import { describe, expect, it } from '@jest/globals'
import { tempDir } from '@pnpm/prepare'

import { executeEffects } from '../src/executeEffects.js'
import { parsePatchFile } from '../src/parsePatch.js'
import { reversePatch } from '../src/reversePatch.js'

interface File {
  contents: string
  mode: number
}

type Files = Record<string, File>

interface RoundTripCase {
  cleanFiles: Files
  modifiedFiles: Files
}

const file = (contents: string, mode = 0o644): File => ({ contents, mode })

// The file names and contents are random, from patch-package's generative tests.
// cspell:disable
const cases: RoundTripCase[] = [
  {
    cleanFiles: { blah: file('\n\n\na\nb\rc\rd\ne\nf\ng\rh\ni\n') },
    modifiedFiles: { blah: file('z\n\na\nb\rc\rd\ne\nf\ng\rh\ni\n_') },
  },
  {
    cleanFiles: { blah: file('a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl') },
    modifiedFiles: { blah: file('d\ne\nf\ng\nh\ni\nj\nk\nm\nl') },
  },
  { cleanFiles: { b: file('\n') }, modifiedFiles: { b: file('') } },
  { cleanFiles: { b: file('') }, modifiedFiles: { b: file('\n') } },
  { cleanFiles: { 'qc-s.4me': file('a\nl\nb\nG') }, modifiedFiles: { 'qc-s.4me': file('\na\nl\nb\nG') } },
  { cleanFiles: { banana: file('\r') }, modifiedFiles: { banana: file('') } },
  { cleanFiles: { f: file('5\n') }, modifiedFiles: { f: file('5\n7\n') } },
  { cleanFiles: { nugs: file('a') }, modifiedFiles: { nugs: file('a\n\n') } },
  { cleanFiles: { b: file('\n') }, modifiedFiles: { b: file('ba\n') } },
  { cleanFiles: { banana: file('WMo^') }, modifiedFiles: { banana: file('\n\n') } },
  { cleanFiles: { b: file('a') }, modifiedFiles: { b: file('a'), c: file('a\n') } },
  {
    cleanFiles: {
      'c-qZ0Qznn1.RWOZ': file('$xs\rwim\t}pJ(;£BZxc\\bg9k|zvBufcaa'),
      'tK/NEDQ-hff.iaQK': file(';4l'),
      'KbYXh8-Dk3J/vcjQ.mz': file('+4:'),
      'r6LXXaS/DO3VbFBswE6.WmHQ': file('rX]bnT%j+,\t\r~xc&`lLh^\\n*-J$z<4xu'),
      'Fa/lQgW3c/G8LsUj-YFoS.4hoY': file('NS'),
    },
    modifiedFiles: {
      'c-qZ0Qznn1.RWOZ': file('$xs\rwim\t}pJ(;£BZxc\\bg9k|zvBufcaa'),
      'tK/NEDQ-hff.iaQK': file(';4l'),
      'KbYXh8-Dk3J/vcjQ.mz': file('+4:'),
      'r6LXXaS/DO3VbFBswE6.WmHQ': file(''),
      'Fa/lQgW3c/G8LsUj-YFoS.4hoY': file('NS'),
      wW1UMkaGn: file('F'),
    },
  },
  {
    cleanFiles: { banana: file('M_7P /c$Y%ldTF=o\nKv_caoM|A\rZ^i!+') },
    modifiedFiles: { banana: file('B-§s\r\nM_7P /c$Y%ldTF=o\nKv_caoM|A\rZ^i!+'), jimmy: file('') },
  },
  { cleanFiles: { 'QBgzpme/jN/Rvr8SP1gZ.9': file('Zk$@') }, modifiedFiles: { 'QBgzpme/jN/Rvr8SP1gZ.9': file('.6f\n7tD*\nZk$@') } },
  { cleanFiles: { '1dkfI.J': file('lineend\n') }, modifiedFiles: { '1dkfI.J': file('nout') } },
  { cleanFiles: { '1dkfI.J': file('a\nb\nc') }, modifiedFiles: { '1dkfI.J': file('b\nb\nc') } },
  { cleanFiles: { 'k/1dt4myqe.e1': file('a\n\n\n\nbanana\n') }, modifiedFiles: { 'k/1dt4myqe.e1': file('b\n\n\n\nbanana\n') } },
  { cleanFiles: { abc: file('E\n') }, modifiedFiles: { cpz: file('E\n') } },
  { cleanFiles: {}, modifiedFiles: { ww: file('banana') } },
  { cleanFiles: { 'bin/run': file('#!/bin/sh\necho 1\n') }, modifiedFiles: { 'bin/run': file('#!/bin/sh\necho 2\n', 0o755) } },
  { cleanFiles: { tool: file('x\n', 0o755) }, modifiedFiles: { tool: file('x\n') } },
  { cleanFiles: { 'src/a.txt': file('one\ntwo\nthree\n') }, modifiedFiles: { 'lib/b.txt': file('one\ntwo\nthree\n') } },
  { cleanFiles: { 'index.js': file('module.exports = f;') }, modifiedFiles: { 'index.js': file('module.exports = f;\nfunction g() {\n}') } },
]
// cspell:enable

// Windows has no executable bit to compare.
const comparesModes = process.platform !== 'win32'

describe.each(cases.map((testCase, index) => [index, testCase] as const))('round trip %i', (_index, testCase) => {
  const patch = diffFiles(testCase)

  it('parses the same with blank context lines stripped of their space', () => {
    const withBlankLines = patch.split('\n').map((line) => line === ' ' ? '' : line).join('\n')
    expect(parsePatchFile(withBlankLines)).toEqual(parsePatchFile(patch))
  })

  it('turns the clean files into the modified files', () => {
    const dir = writeFiles(testCase.cleanFiles)
    executeEffects(parsePatchFile(patch), { root: dir, dryRun: false })
    expect(readFiles(dir)).toEqual(expectedFiles(testCase.modifiedFiles))
  })

  it('turns the modified files back into the clean files', () => {
    const dir = writeFiles(testCase.modifiedFiles)
    executeEffects(reversePatch(parsePatchFile(patch)), { root: dir, dryRun: false })
    expect(readFiles(dir)).toEqual(expectedFiles(testCase.cleanFiles))
  })
})

function diffFiles (testCase: RoundTripCase): string {
  const repo = writeFiles(testCase.cleanFiles)
  git(repo, 'init', '-q')
  git(repo, 'add', '-A')
  git(repo, 'commit', '-q', '--allow-empty', '-m', 'clean')
  git(repo, 'rm', '-q', '-r', '--ignore-unmatch', '.') // cspell:disable-line
  writeFilesTo(repo, testCase.modifiedFiles)
  git(repo, 'add', '-A')
  return git(repo, 'diff', '--cached', '--no-color')
}

function git (cwd: string, ...args: string[]): string {
  return execFileSync('git', [
    '-c', 'user.name=test',
    '-c', 'user.email=test@example.com',
    '-c', 'core.autocrlf=false',
    '-c', 'core.fileMode=true',
    ...args,
  ], { cwd, encoding: 'utf8' })
}

function writeFiles (files: Files): string {
  const dir = tempDir(false)
  writeFilesTo(dir, files)
  return dir
}

function writeFilesTo (dir: string, files: Files): void {
  for (const [filePath, { contents, mode }] of Object.entries(files)) {
    const target = path.join(dir, filePath)
    fs.mkdirSync(path.dirname(target), { recursive: true })
    fs.writeFileSync(target, contents)
    fs.chmodSync(target, mode)
  }
}

function readFiles (dir: string): Files {
  const files: Files = {}
  for (const entry of fs.readdirSync(dir, { recursive: true, withFileTypes: true })) {
    const relativePath = path.relative(dir, path.join(entry.parentPath, entry.name)).split(path.sep).join('/')
    if (!entry.isFile() || relativePath.startsWith('.git/')) continue
    const absolutePath = path.join(dir, relativePath)
    files[relativePath] = {
      contents: fs.readFileSync(absolutePath, 'utf8'),
      mode: comparesModes ? fs.statSync(absolutePath).mode & 0o777 : 0,
    }
  }
  return files
}

function expectedFiles (files: Files): Files {
  return Object.fromEntries(Object.entries(files).map(([filePath, { contents, mode }]) => [filePath, { contents, mode: comparesModes ? mode : 0 }]))
}
