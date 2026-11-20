import { expect, test } from '@jest/globals'

import { renderLicences } from '../../src/licenses/outputRenderer.js'

const clearScreen = '\u001B[2J'
const redText = '\u001B[31m'

test('terminal license metadata cannot emit control sequences', () => {
  const metadata = {
    belongsTo: 'dependencies' as const,
    name: 'package',
    version: '1.0.0',
    license: clearScreen + 'MIT',
    author: '\u001B]8;;https://evil.example\u0007author\u001B]8;;\u0007',
    description: redText + 'description\u001B[0m\nsecond line',
    homepage: 'https://example.com/\u009B2J',
    path: '/package',
  }
  const { output } = renderLicences([metadata], { long: true })
  expect(output).not.toContain('\u0007')
  expect(output).not.toContain('\u009B')
  expect(output).not.toContain('\u001B[2J')
  expect(output).not.toContain('\u001B]8;')
  expect(output).toContain('MIT')
  expect(output).toContain('author')
  expect(output).toContain('description')
  expect(output).toContain('second line')
})

test('JSON license output preserves metadata as escaped data', () => {
  const metadata = { belongsTo: 'dependencies' as const, name: 'package', version: '1.0.0', license: clearScreen + 'MIT', path: '/package' }
  const { output } = renderLicences([metadata], { json: true })
  expect(JSON.parse(output)[metadata.license][0].license).toBe(metadata.license)
  expect(output).not.toContain('\u001B')
})
