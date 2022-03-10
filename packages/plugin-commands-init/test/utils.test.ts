import { workWithInitModule, personToString } from '@pnpm/plugin-commands-init/lib/utils'

test('run the workWithInitModule function', () => {
  const rawConfig = {
    initVersion: '2.0.0',
    initModule: '~/.pnpm-init.js',
  }
  expect(workWithInitModule(rawConfig)).toEqual({
    initVersion: '2.0.0',
  })
})

test('run the personToString function', () => {
  const expectAuthor = 'pnpm <xxxxxx@pnpm.com> (https://www.github.com/pnpm)'
  expect(personToString({
    email: 'xxxxxx@pnpm.com',
    name: 'pnpm',
    url: 'https://www.github.com/pnpm',
  })).toBe(expectAuthor)
  expect(personToString(expectAuthor)).toBe(expectAuthor)
})
