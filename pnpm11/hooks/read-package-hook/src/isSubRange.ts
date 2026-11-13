import semver from 'semver'

const MIN_RELEASE_FLOOR = new semver.Comparator('>=0.0.0')

export function isSubRange (superRange: string | undefined, subRange: string | undefined): boolean {
  const trimmedSuper = superRange?.trim()
  if (!trimmedSuper) return true
  const trimmedSub = subRange?.trim()
  if (!trimmedSub) return false
  if (trimmedSub === trimmedSuper) return true
  if (semver.validRange(trimmedSub) == null || semver.validRange(trimmedSuper) == null) {
    return false
  }
  const parsedSuper = new semver.Range(trimmedSuper)
  return trimmedSub.split('||').every((rawBranch) => {
    const branch = rawBranch.trim()
    if (branch === '') {
      return semver.subset('*', parsedSuper)
    }
    const exactSubVersion = semver.clean(branch)
    if (exactSubVersion != null) {
      return semver.satisfies(exactSubVersion, parsedSuper)
    }
    return semver.subset(withImplicitReleaseFloor(new semver.Range(branch)), parsedSuper)
  })
}

function withImplicitReleaseFloor (range: semver.Range): semver.Range {
  range.set = range.set.map((comparators) =>
    needsImplicitReleaseFloor(comparators) ? [...comparators, MIN_RELEASE_FLOOR] : comparators
  )
  return range
}

function needsImplicitReleaseFloor (comparators: readonly semver.Comparator[]): boolean {
  if (comparators.length === 0) return false
  const isUpperOnly = comparators.every((comparator) => comparator.operator === '<' || comparator.operator === '<=')
  if (!isUpperOnly) return false
  return !comparators.some((comparator) =>
    comparator.semver.major === 0 &&
    comparator.semver.minor === 0 &&
    comparator.semver.patch === 0 &&
    comparator.semver.prerelease.length > 0 &&
    !(comparator.operator === '<' && comparator.semver.prerelease.length === 1 && comparator.semver.prerelease[0] === 0)
  )
}
