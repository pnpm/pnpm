/**
 * Mock git checks
 */
const getCurrentBranchName = jest.fn()
jest.mock('@pnpm/lockfile-file/lib/gitChecks', () => {
  const original = jest.requireActual('@pnpm/lockfile-file/lib/gitChecks')
  return {
    ...original,
    getCurrentBranchName,
  }
})

export {
  getCurrentBranchName,
}