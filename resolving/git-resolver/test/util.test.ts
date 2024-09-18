import { AssertionError } from 'assert'
import { getCommitSha, getAllRefs, isSsh, resolveTags, retry } from '../src/util'
import semver from 'semver'
import { execSync } from 'child_process'

describe('retry()', () => {
  test('Throws if no retries are specified', () => {
    expect(() => retry(() => {
      throw new Error()
    }, 0)).toThrow(AssertionError)
  })

  test('Throws if all retries are exausted', () => {
    expect(() => retry(() => {
      throw new Error()
    }, 2)).toThrow(Error)
  })

  test.each([1, 2, 3])('Returns target value after exactly %s of 3 attempts', (attempt) => {
    let count = 0
    const fn = jest.fn(() => {
      if (count === attempt - 1) {
        return 'ok'
      } else {
        count++
        throw new Error()
      }
    })
    expect(retry(fn, 3)).toBe('ok')
    expect(fn).toHaveBeenCalledTimes(attempt)
  })
})

describe('resolveVTags()', () => {
  test('Binds to semver.maxSatisfying with loose evaluation', () => {
    const maxSatisfyingSpy = jest.spyOn(semver, 'maxSatisfying')
    const vTags: string[] = []
    const range = ''
    resolveTags(vTags, range)
    expect(maxSatisfyingSpy).toHaveBeenCalledWith(vTags, range, true)
  })

  test('Returns undefined if no tag is found', () => {
    expect(resolveTags([], '')).toBeUndefined()
  })
})

describe('isSsh()', () => {
  test.each([
    { input: 'git+ssh://test.com', output: true },
    { input: 'ssh://test.com', output: true },
    { input: 'git@test.com', output: true },
    { input: 'https://git@test.com', output: false },
    { input: 'git+ftp://test.com', output: false },
    { input: 'ss://test.com', output: false },
  ])('Returns $output for $input', ({ input, output }) => {
    expect(isSsh(input)).toBe(output)
  })
})

jest.mock('child_process')
const execMock = jest.mocked(execSync)

describe('getRefs()', () => {
  beforeEach(() => execMock.mockReset())

  test('Maps newline-delimited entries into tab-delimited key-value pairs (reversed)', () => {
    execMock.mockReturnValue('sha1\tbranch1\nsha2\tbranch2')

    expect(getAllRefs('repo')).toStrictEqual({
      branch1: 'sha1',
      branch2: 'sha2',
    })
  })

  test('Retries exactly once after initial failure', () => {
    execMock
      .mockImplementationOnce(() => {
        throw new Error()
      }).mockImplementationOnce(() => 'sha\tmain')

    expect(getAllRefs('repo')).toHaveProperty('main', 'sha')
    expect(execSync).toHaveBeenCalledTimes(2)
  })

  test('Maps args of', () => {
    execMock.mockReturnValue('sha\tmain')
    getAllRefs('abc')
    expect(execMock).toHaveBeenCalledWith('git ls-remote abc', { encoding: 'utf8' })
  })
})

describe('getCommitSha()', () => {
  beforeEach(() => execMock.mockReset())

  test('Throws if repo contains spaces', () => {
    expect(() => getCommitSha('abc def', '123')).toThrow(AssertionError)
  })

  test('Throws if ref contains spaces', () => {
    expect(() => getCommitSha('abc', '123 456')).toThrow(AssertionError)
  })

  test('Maps args of', () => {
    execMock.mockReturnValue('sha\tmain')
    const ref = getCommitSha('abc', '123')
    expect(execMock).toHaveBeenCalledWith('git ls-remote abc 123', { encoding: 'utf8' })
    expect(ref).toBe('sha')
  })
})
