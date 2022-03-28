import { getPref } from '@pnpm/manifest-utils'

test('getPref()', () => {
  expect(getPref('foo', 'foo', '1.0.0', {})).toEqual('^1.0.0')

  expect(
    getPref('foo', 'foo', '1.0.0', {
      pinnedVersion: 'major',
    })
  ).toEqual('^1.0.0')

  expect(
    getPref('foo', 'foo', '2.0.0', {
      pinnedVersion: 'minor',
    })
  ).toEqual('~2.0.0')

  expect(
    getPref('foo', 'foo', '3.0.0', {
      pinnedVersion: 'patch',
    })
  ).toEqual('3.0.0')

  expect(
    getPref('foo', 'foo', '4.0.0', {
      pinnedVersion: 'none',
    })
  ).toEqual('*')

  expect(
    getPref('foo', 'foo', undefined, {
      pinnedVersion: 'major',
    })
  ).toEqual('*')
})
