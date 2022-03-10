import { workWithInitModule, unParsePerson, Person } from '@pnpm/plugin-commands-init/lib/utils'

test('run the workWithInitModule function', () => {
  const rawConfig = {
    initVersion: '2.0.0',
    initModule: '~/.pnpm-init.js',
  }
  expect(workWithInitModule(rawConfig)).toEqual({
    initVersion: '2.0.0',
  })
})

test('run the unParsePerson function', () => {
  const person: Person = {
    email: 'xxxxxx@pnpm.com',
    name: 'pnpm',
    url: 'https://www.github.com/pnpm',
  }
  const expectAuthor = 'pnpm <xxxxxx@pnpm.com> (https://www.github.com/pnpm)'
  expect(unParsePerson(person)).toBe(expectAuthor)
})
