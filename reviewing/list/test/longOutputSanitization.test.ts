import fs from 'fs'
import os from 'os'
import path from 'path'
import { stripVTControlCharacters as stripAnsi } from 'util'
import { renderDependentsTree } from '../lib/renderDependentsTree.js'
import { renderTree } from '../lib/renderTree.js'

const DESCRIPTION = '\u001B[2J\u001B[H' + 'first line\nsecond line'
const REPOSITORY = '\u001B]52;c;cHduZWQ=\u0007https://example.com/repo'
const HOMEPAGE = 'https://example.com/\u009B2J'

let pkgDir: string

beforeAll(() => {
  pkgDir = fs.mkdtempSync(path.join(os.tmpdir(), 'list-long-'))
  fs.writeFileSync(path.join(pkgDir, 'package.json'), JSON.stringify({
    name: 'foo',
    version: '1.0.0',
    description: DESCRIPTION,
    repository: REPOSITORY,
    homepage: HOMEPAGE,
  }))
})

afterAll(() => {
  fs.rmSync(pkgDir, { recursive: true, force: true })
})

function expectSanitizedManifestFields (output: string): void {
  expect(output).not.toContain('\u001B[2J')
  expect(output).not.toContain('\u001B]52;')
  expect(output).not.toContain('\u0007')
  expect(output).not.toContain('\u009B')
  const lines = stripAnsi(output).split('\n').map((line) => line.replace(/^[\s│├└─]+/, ''))
  expect(lines).toContain('[2J[Hfirst line')
  expect(lines).toContain('second line')
  expect(lines).toContain(']52;c;cHduZWQ=https://example.com/repo')
  expect(lines).toContain('https://example.com/2J')
}

test('pnpm list --long strips terminal control characters from manifest fields', async () => {
  const output = await renderTree([
    {
      name: 'root',
      version: '0.0.0',
      path: path.dirname(pkgDir),
      dependencies: [
        {
          alias: 'foo',
          name: 'foo',
          version: '1.0.0',
          path: pkgDir,
          isPeer: false,
          isSkipped: false,
          isMissing: false,
        },
      ],
    },
  ], { alwaysPrintRootPackage: true, depth: 0, long: true, search: false, showExtraneous: false })

  expectSanitizedManifestFields(output)
})

test('pnpm why --long strips terminal control characters from manifest fields', async () => {
  const output = await renderDependentsTree([
    {
      name: 'foo',
      version: '1.0.0',
      path: pkgDir,
      dependents: [
        { name: 'my-project', version: '0.0.0', depField: 'dependencies' },
      ],
    },
  ], { long: true })

  expectSanitizedManifestFields(output)
})
