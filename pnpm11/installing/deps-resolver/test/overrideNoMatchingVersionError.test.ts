import { expect, test } from '@jest/globals'
import type { VersionOverride } from '@pnpm/config.parse-overrides'

import { toOverrideNoMatchingVersionError } from '../src/overrideNoMatchingVersionError.js'

const noMatchingVersion = {
  code: 'ERR_PNPM_NO_MATCHING_VERSION',
  packageMeta: { versions: { '1.0.0': {}, '1.1.0': {}, '2.0.0': {} } },
}

function override (selectorRange: string | undefined, newBareSpecifier: string): VersionOverride {
  return {
    selector: selectorRange ? `acme@${selectorRange}` : 'acme',
    targetPkg: { name: 'acme', bareSpecifier: selectorRange },
    newBareSpecifier,
  }
}

test('names the override and the latest version in its selector range', () => {
  const err = toOverrideNoMatchingVersionError(noMatchingVersion, { alias: 'acme', bareSpecifier: '1.9.0' }, [override('^1', '1.9.0')])

  expect(err?.message).toBe('Override "acme@^1": "1.9.0" targets a version of acme that does not exist on the registry.')
  expect(err?.hint).toBe('The latest release of acme matching "^1" is "1.1.0".')
})

test('omits the version hint when the override forces an alias of another package', () => {
  const err = toOverrideNoMatchingVersionError(noMatchingVersion, { alias: 'acme', bareSpecifier: 'npm:other@9.0.0' }, [override('^1', 'npm:other@9.0.0')])

  expect(err?.message).toBe('Override "acme@^1": "npm:other@9.0.0" targets a version of acme that does not exist on the registry.')
  expect(err?.hint).toBeUndefined()
})

test('keeps the hint the original error carried', () => {
  const lockfileHint = 'This optional dependency is not skipped, because the lockfile contains a resolution for it.'
  const err = toOverrideNoMatchingVersionError({ ...noMatchingVersion, hint: lockfileHint }, { alias: 'acme', bareSpecifier: '1.9.0' }, [override('^1', '1.9.0')])

  expect(err?.hint).toBe(`The latest release of acme matching "^1" is "1.1.0".\n\n${lockfileHint}`)
})

test('returns null when no override forced the failing specifier', () => {
  expect(toOverrideNoMatchingVersionError(noMatchingVersion, { alias: 'acme', bareSpecifier: '1.9.0' }, [override('^1', '3.0.0')])).toBeNull()
})
