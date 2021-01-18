/// <reference path="../../../typings/index.d.ts"/>
import exportableManifest from '@pnpm/exportable-manifest'

test('the pnpm options are removed', async () => {
  expect(await exportableManifest(process.cwd(), {
    name: 'foo',
    version: '1.0.0',
    dependencies: {
      qar: '2',
    },
    pnpm: {
      overrides: {
        bar: '1',
      },
    },
  })).toStrictEqual({
    name: 'foo',
    version: '1.0.0',
    dependencies: {
      qar: '2',
    },
  })
})
