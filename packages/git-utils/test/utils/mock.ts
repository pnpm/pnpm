/**
 * Mock git utils
 */

const originalAPI = jest.requireActual('@pnpm/git-utils')

const isGitRepo = jest.fn().mockImplementationOnce(originalAPI.isGitRepo)
const getCurrentBranch = jest.fn().mockImplementation(originalAPI.getCurrentBranch)
const isWorkingTreeClean = jest.fn().mockImplementationOnce(originalAPI.isWorkingTreeClean)
const isRemoteHistoryClean = jest.fn().mockImplementation(originalAPI.isRemoteHistoryClean)

jest.mock('@pnpm/git-utils', () => {
  return {
    ...originalAPI,
    isGitRepo,
    getCurrentBranch,
    isWorkingTreeClean,
    isRemoteHistoryClean,
  }
})

export {
  isGitRepo,
  getCurrentBranch,
  isWorkingTreeClean,
  isRemoteHistoryClean,
  originalAPI,
}