import semver from 'semver'

type ReleaseTuple = readonly [number, number, number]
type ReleasePoint = readonly [number, number, bigint]
type PreId = number | string
type PrePoint = readonly PreId[]

interface Interval<Point> {
  low: Point
  high: Point | null
}

interface SemverSelectorRange {
  kind: 'semver'
  releaseIntervals: Array<Interval<ReleasePoint>>
  prereleaseIntervals: Map<string, Array<Interval<PrePoint>>>
}

export type ParsedSelectorRange =
  | { kind: 'unbounded' }
  | { kind: 'non-semver', raw: string }
  | SemverSelectorRange

export function isSubRange (superRange: string | undefined, subRange: string | undefined): boolean {
  return selectorRangeCovers(parseSelectorRange(superRange), parseSelectorRange(subRange))
}

export function parseSelectorRange (range: string | undefined): ParsedSelectorRange {
  const trimmed = range?.trim()
  if (!trimmed) return { kind: 'unbounded' }
  if (semver.validRange(trimmed) == null) return { kind: 'non-semver', raw: trimmed }
  return parseValidSemverRange(trimmed)
}

export function selectorRangeCovers (broad: ParsedSelectorRange, narrow: ParsedSelectorRange): boolean {
  if (broad.kind === 'unbounded') return true
  if (narrow.kind === 'unbounded') return false
  if (broad.kind === 'non-semver') return narrow.kind === 'non-semver' && broad.raw === narrow.raw
  if (narrow.kind === 'non-semver') return false
  return semverIntervalsCover(broad, narrow)
}

function semverIntervalsCover (dom: SemverSelectorRange, sub: SemverSelectorRange): boolean {
  if (!intervalsCover(dom.releaseIntervals, sub.releaseIntervals, compareReleasePoint)) {
    return false
  }
  for (const [tupleKey, subIntervals] of sub.prereleaseIntervals) {
    const domIntervals = dom.prereleaseIntervals.get(tupleKey)
    if (!domIntervals || !intervalsCover(domIntervals, subIntervals, comparePrePoint)) {
      return false
    }
  }
  return true
}

function parseValidSemverRange (trimmed: string): SemverSelectorRange {
  const releaseRaw: Array<Interval<ReleasePoint>> = []
  const prereleaseRaw = new Map<string, Array<Interval<PrePoint>>>()
  for (const rawBranch of trimmed.split('||')) {
    const branch = rawBranch.trim()
    const branchRange = new semver.Range(branch === '' ? '*' : branch)
    for (const comparators of branchRange.set) {
      collectConjunctionIntervals(comparators, releaseRaw, prereleaseRaw)
    }
  }
  const prereleaseIntervals = new Map<string, Array<Interval<PrePoint>>>()
  for (const [tupleKey, intervals] of prereleaseRaw) {
    const merged = coalesceIntervals(intervals, comparePrePoint)
    if (merged.length > 0) {
      prereleaseIntervals.set(tupleKey, merged)
    }
  }
  return {
    kind: 'semver',
    releaseIntervals: coalesceIntervals(releaseRaw, compareReleasePoint),
    prereleaseIntervals,
  }
}

function collectConjunctionIntervals (
  comparators: readonly semver.Comparator[],
  releaseRaw: Array<Interval<ReleasePoint>>,
  prereleaseRaw: Map<string, Array<Interval<PrePoint>>>
): void {
  const releaseInterval = conjunctionReleaseInterval(comparators)
  if (releaseInterval != null) {
    releaseRaw.push(releaseInterval)
  }
  for (const [tupleKey, preInterval] of conjunctionPrereleaseIntervals(comparators)) {
    const list = prereleaseRaw.get(tupleKey)
    if (list != null) {
      list.push(preInterval)
    } else {
      prereleaseRaw.set(tupleKey, [preInterval])
    }
  }
}

function conjunctionReleaseInterval (
  comparators: readonly semver.Comparator[]
): Interval<ReleasePoint> | undefined {
  let current: Interval<ReleasePoint> = { low: [0, 0, 0n], high: null }
  for (const comparator of comparators) {
    const next = applyReleaseComparator(current, comparator)
    if (next == null) return undefined
    current = next
  }
  return isNonEmpty(current, compareReleasePoint) ? current : undefined
}

function applyReleaseComparator (
  current: Interval<ReleasePoint>,
  comparator: semver.Comparator
): Interval<ReleasePoint> | undefined {
  if (comparator.value === '') return current
  const { major, minor, patch, prerelease } = comparator.semver
  const hasPre = prerelease.length > 0
  const point: ReleasePoint = [major, minor, BigInt(patch)]
  const nextPoint: ReleasePoint = [major, minor, BigInt(patch) + 1n]
  return narrowReleaseBounds(current, comparator.operator, hasPre, point, nextPoint)
}

function narrowReleaseBounds (
  current: Interval<ReleasePoint>,
  operator: string,
  hasPre: boolean,
  point: ReleasePoint,
  nextPoint: ReleasePoint
): Interval<ReleasePoint> | undefined {
  if (operator === '' || operator === '=') {
    if (hasPre) return undefined
    return {
      low: maxPoint(current.low, point, compareReleasePoint),
      high: minHigh(current.high, nextPoint, compareReleasePoint),
    }
  }
  if (operator === '>=' || operator === '>') {
    const floor = releaseFloor(operator, hasPre, point, nextPoint)
    return { low: maxPoint(current.low, floor, compareReleasePoint), high: current.high }
  }
  const ceiling = releaseCeiling(operator, hasPre, point, nextPoint)
  return { low: current.low, high: minHigh(current.high, ceiling, compareReleasePoint) }
}

function releaseFloor (operator: string, hasPre: boolean, point: ReleasePoint, nextPoint: ReleasePoint): ReleasePoint {
  return operator === '>' && !hasPre ? nextPoint : point
}

function releaseCeiling (operator: string, hasPre: boolean, point: ReleasePoint, nextPoint: ReleasePoint): ReleasePoint {
  return operator === '<=' && !hasPre ? nextPoint : point
}

function conjunctionPrereleaseIntervals (
  comparators: readonly semver.Comparator[]
): Array<[string, Interval<PrePoint>]> {
  const tuples = collectPrereleaseTuples(comparators)
  const results: Array<[string, Interval<PrePoint>]> = []
  for (const [tupleKey, tuple] of tuples) {
    const interval = tuplePrereleaseInterval(tuple, comparators)
    if (interval != null) {
      results.push([tupleKey, interval])
    }
  }
  return results
}

function collectPrereleaseTuples (comparators: readonly semver.Comparator[]): Map<string, ReleaseTuple> {
  const tuples = new Map<string, ReleaseTuple>()
  for (const comparator of comparators) {
    if (comparator.value !== '' && comparator.semver.prerelease.length > 0) {
      const { major, minor, patch } = comparator.semver
      tuples.set(`${major}.${minor}.${patch}`, [major, minor, patch])
    }
  }
  return tuples
}

function tuplePrereleaseInterval (
  target: ReleaseTuple,
  comparators: readonly semver.Comparator[]
): Interval<PrePoint> | undefined {
  let current: Interval<PrePoint> = { low: [0], high: null }
  for (const comparator of comparators) {
    const next = applyPrereleaseComparator(target, current, comparator)
    if (next == null) return undefined
    current = next
  }
  return isNonEmpty(current, comparePrePoint) ? current : undefined
}

function applyPrereleaseComparator (
  target: ReleaseTuple,
  current: Interval<PrePoint>,
  comparator: semver.Comparator
): Interval<PrePoint> | undefined {
  if (comparator.value === '') return current
  const { major, minor, patch, prerelease } = comparator.semver
  const tupleCmp = compareTuple([major, minor, patch], target)
  if (tupleCmp !== 0 || prerelease.length === 0) {
    return admitsNonMatchingTuple(tupleCmp, comparator.operator) ? current : undefined
  }
  return narrowSameTuplePrerelease(current, comparator.operator, prerelease)
}

function admitsNonMatchingTuple (tupleCmp: number, operator: string): boolean {
  return tupleCmp < 0
    ? operator === '>' || operator === '>='
    : operator === '<' || operator === '<='
}

function narrowSameTuplePrerelease (
  current: Interval<PrePoint>,
  operator: string,
  pre: PrePoint
): Interval<PrePoint> {
  const nextPre: PrePoint = [...pre, 0]
  if (operator === '' || operator === '=') {
    return { low: maxPoint(current.low, pre, comparePrePoint), high: minHigh(current.high, nextPre, comparePrePoint) }
  }
  if (operator === '>=' || operator === '>') {
    const floor = operator === '>' ? nextPre : pre
    return { low: maxPoint(current.low, floor, comparePrePoint), high: current.high }
  }
  const ceiling = operator === '<=' ? nextPre : pre
  return { low: current.low, high: minHigh(current.high, ceiling, comparePrePoint) }
}

function isNonEmpty<Point> (interval: Interval<Point>, compare: (lhs: Point, rhs: Point) => number): boolean {
  return interval.high === null || compare(interval.low, interval.high) < 0
}

function maxPoint<Point> (lhs: Point, rhs: Point, compare: (lhs: Point, rhs: Point) => number): Point {
  return compare(lhs, rhs) >= 0 ? lhs : rhs
}

function minHigh<Point> (lhs: Point | null, rhs: Point, compare: (lhs: Point, rhs: Point) => number): Point {
  if (lhs === null) return rhs
  return compare(lhs, rhs) <= 0 ? lhs : rhs
}

function compareUpper<Point> (lhs: Point | null, rhs: Point | null, compare: (lhs: Point, rhs: Point) => number): number {
  if (lhs === rhs) return 0
  if (lhs === null) return 1
  if (rhs === null) return -1
  return compare(lhs, rhs)
}

function coalesceIntervals<Point> (
  intervals: ReadonlyArray<Interval<Point>>,
  compare: (lhs: Point, rhs: Point) => number
): Array<Interval<Point>> {
  const sorted = [...intervals].sort((lhs, rhs) => compare(lhs.low, rhs.low))
  const merged: Array<Interval<Point>> = []
  for (const next of sorted) {
    const current = merged[merged.length - 1]
    if (current != null && (current.high === null || compare(next.low, current.high) <= 0)) {
      if (compareUpper(next.high, current.high, compare) > 0) {
        current.high = next.high
      }
    } else {
      merged.push({ low: next.low, high: next.high })
    }
  }
  return merged
}

function intervalsCover<Point> (
  dom: ReadonlyArray<Interval<Point>>,
  sub: ReadonlyArray<Interval<Point>>,
  compare: (lhs: Point, rhs: Point) => number
): boolean {
  return sub.every((subIv) =>
    dom.some((domIv) =>
      compare(domIv.low, subIv.low) <= 0 && compareUpper(subIv.high, domIv.high, compare) <= 0
    )
  )
}

function compareTuple (lhs: ReleaseTuple, rhs: ReleaseTuple): number {
  if (lhs[0] !== rhs[0]) return lhs[0] - rhs[0]
  if (lhs[1] !== rhs[1]) return lhs[1] - rhs[1]
  return lhs[2] - rhs[2]
}

function compareReleasePoint (lhs: ReleasePoint, rhs: ReleasePoint): number {
  if (lhs[0] !== rhs[0]) return lhs[0] - rhs[0]
  if (lhs[1] !== rhs[1]) return lhs[1] - rhs[1]
  if (lhs[2] === rhs[2]) return 0
  return lhs[2] < rhs[2] ? -1 : 1
}

function comparePrePoint (lhs: PrePoint, rhs: PrePoint): number {
  const sharedLen = Math.min(lhs.length, rhs.length)
  for (let idx = 0; idx < sharedLen; idx++) {
    const diff = comparePreId(lhs[idx], rhs[idx])
    if (diff !== 0) return diff
  }
  return lhs.length - rhs.length
}

function comparePreId (lhs: PreId, rhs: PreId): number {
  if (lhs === rhs) return 0
  if (typeof lhs === 'number' && typeof rhs === 'number') return lhs - rhs
  if (typeof lhs === 'number') return -1
  if (typeof rhs === 'number') return 1
  return lhs < rhs ? -1 : 1
}
