import { describe, expect, test } from '@jest/globals'
import type { LicensePackage } from '@pnpm/deps.compliance.license-scanner'

import { renderDetails } from '../../src/licenses/outputRenderer.js'

describe('renderDetails', () => {
  test('sanitizes each field but preserves the newline joins', () => {
    const pkg = {
      belongsTo: 'dependencies',
      name: 'evil-pkg',
      version: '1.0.0',
      license: 'MIT',
      author: 'Evil\x1b[2JAuthor',
      description: 'A description',
      homepage: 'http://ex\x9bample.com',
    } as LicensePackage
    // ANSI CSI in the author and the 8-bit C1 byte in the homepage are stripped,
    // while the '\n' separators between the three detail fields survive.
    expect(renderDetails(pkg)).toBe('EvilAuthor\nA description\nhttp://example.com')
  })
})
