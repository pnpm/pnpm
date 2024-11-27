import path from 'path'
import { getPnpmfilePath } from './getPnpmfilePath'

test('getPnpmfilePath() when pnpmfile is undefined', () => {
  expect(getPnpmfilePath('PREFIX', undefined)).toBe(path.join('PREFIX', '.pnpmfile.cjs'))
})

test('getPnpmfilePath() when pnpmfile is a relative path', () => {
  expect(getPnpmfilePath('PREFIX', 'hooks/pnpm.js')).toBe(path.join('PREFIX', 'hooks/pnpm.js'))
})

test('getPnpmfilePath() when pnpmfile is an absolute path', () => {
  expect(getPnpmfilePath('PREFIX', '/global/pnpmfile.cjs')).toBe('/global/pnpmfile.cjs')
})
