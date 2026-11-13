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

interface TupleWindow {
  exclusiveFloor: ReleaseTuple | null
  inclusiveFloor: ReleaseTuple | null
  inclusiveCeiling: ReleaseTuple | null
  impossible: boolean
}

interface TuplePrereleaseEntry {
  tuple: ReleaseTuple
  bounds: semver.Comparator[]
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
  const parsedRange = new semver.Range(trimmed)
  for (const comparators of parsedRange.set) {
    collectConjunctionIntervals(comparators, releaseRaw, prereleaseRaw)
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
  const operator = comparator.operator
  if (operator === '' || operator === '=') {
    if (comparator.semver.prerelease.length > 0) return undefined
    const point = releasePoint(comparator.semver, 0n)
    return {
      low: maxPoint(current.low, point, compareReleasePoint),
      high: minHigh(current.high, releasePoint(comparator.semver, 1n), compareReleasePoint),
    }
  }
  if (operator === '>=' || operator === '>') {
    return { low: maxPoint(current.low, releaseFloor(comparator), compareReleasePoint), high: current.high }
  }
  return { low: current.low, high: minHigh(current.high, releaseCeiling(comparator), compareReleasePoint) }
}

function releasePoint (version: semver.SemVer, patchDelta: bigint): ReleasePoint {
  return [version.major, version.minor, BigInt(version.patch) + patchDelta]
}

function releaseFloor (comparator: semver.Comparator): ReleasePoint {
  const step = comparator.operator === '>' && comparator.semver.prerelease.length === 0 ? 1n : 0n
  return releasePoint(comparator.semver, step)
}

function releaseCeiling (comparator: semver.Comparator): ReleasePoint {
  const step = comparator.operator === '<=' && comparator.semver.prerelease.length === 0 ? 1n : 0n
  return releasePoint(comparator.semver, step)
}

function conjunctionPrereleaseIntervals (
  comparators: readonly semver.Comparator[]
): Array<[string, Interval<PrePoint>]> {
  const { window, perTuple } = indexPrereleaseBounds(comparators)
  const results: Array<[string, Interval<PrePoint>]> = []
  for (const [tupleKey, entry] of perTuple) {
    if (!admitsTuple(window, entry.tuple)) continue
    const interval = sameTuplePrereleaseInterval(entry.bounds)
    if (interval != null) {
      results.push([tupleKey, interval])
    }
  }
  return results
}

function indexPrereleaseBounds (
  comparators: readonly semver.Comparator[]
): { window: TupleWindow, perTuple: Map<string, TuplePrereleaseEntry> } {
  const window: TupleWindow = {
    exclusiveFloor: null,
    inclusiveFloor: null,
    inclusiveCeiling: null,
    impossible: false,
  }
  const perTuple = new Map<string, TuplePrereleaseEntry>()
  for (const comparator of comparators) {
    if (comparator.value === '') continue
    recordTupleWindow(window, comparator)
    if (comparator.semver.prerelease.length > 0) {
      appendTupleBound(perTuple, comparator)
    }
  }
  return { window, perTuple }
}

function appendTupleBound (perTuple: Map<string, TuplePrereleaseEntry>, comparator: semver.Comparator): void {
  const { major, minor, patch } = comparator.semver
  const key = `${major}.${minor}.${patch}`
  const existing = perTuple.get(key)
  if (existing != null) {
    existing.bounds.push(comparator)
  } else {
    perTuple.set(key, { tuple: [major, minor, patch], bounds: [comparator] })
  }
}

function recordTupleWindow (window: TupleWindow, comparator: semver.Comparator): void {
  const { major, minor, patch, prerelease } = comparator.semver
  const tuple: ReleaseTuple = [major, minor, patch]
  const hasPre = prerelease.length > 0
  const operator = comparator.operator
  if (operator === '' || operator === '=') {
    if (!hasPre) {
      window.impossible = true
      return
    }
    window.inclusiveFloor = raiseFloor(window.inclusiveFloor, tuple)
    window.inclusiveCeiling = lowerCeiling(window.inclusiveCeiling, tuple)
    return
  }
  if (operator === '>' || operator === '>=') {
    if (hasPre) {
      window.inclusiveFloor = raiseFloor(window.inclusiveFloor, tuple)
    } else {
      window.exclusiveFloor = raiseFloor(window.exclusiveFloor, tuple)
    }
    return
  }
  window.inclusiveCeiling = lowerCeiling(window.inclusiveCeiling, tuple)
}

function admitsTuple (window: TupleWindow, target: ReleaseTuple): boolean {
  if (window.impossible) return false
  if (window.exclusiveFloor != null && compareTuple(target, window.exclusiveFloor) <= 0) return false
  if (window.inclusiveFloor != null && compareTuple(target, window.inclusiveFloor) < 0) return false
  return window.inclusiveCeiling == null || compareTuple(target, window.inclusiveCeiling) <= 0
}

function raiseFloor (current: ReleaseTuple | null, candidate: ReleaseTuple): ReleaseTuple {
  if (current == null) return candidate
  return maxPoint(current, candidate, compareTuple)
}

function lowerCeiling (current: ReleaseTuple | null, candidate: ReleaseTuple): ReleaseTuple {
  if (current == null) return candidate
  return compareTuple(current, candidate) <= 0 ? current : candidate
}

function sameTuplePrereleaseInterval (
  bounds: readonly semver.Comparator[]
): Interval<PrePoint> | undefined {
  let current: Interval<PrePoint> = { low: [0], high: null }
  for (const comparator of bounds) {
    current = narrowSameTuplePrerelease(current, comparator)
  }
  return isNonEmpty(current, comparePrePoint) ? current : undefined
}

function narrowSameTuplePrerelease (
  current: Interval<PrePoint>,
  comparator: semver.Comparator
): Interval<PrePoint> {
  const operator = comparator.operator
  const pre = comparator.semver.prerelease
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

function endsAtOrBefore<Point> (
  high: Point | null,
  point: Point,
  compare: (lhs: Point, rhs: Point) => number
): boolean {
  return high !== null && compare(high, point) <= 0
}

function intervalsCover<Point> (
  dom: ReadonlyArray<Interval<Point>>,
  sub: ReadonlyArray<Interval<Point>>,
  compare: (lhs: Point, rhs: Point) => number
): boolean {
  let domIndex = 0
  for (const subInterval of sub) {
    while (domIndex < dom.length && endsAtOrBefore(dom[domIndex].high, subInterval.low, compare)) {
      domIndex++
    }
    const domInterval = dom[domIndex]
    if (
      domInterval == null ||
      compare(domInterval.low, subInterval.low) > 0 ||
      compareUpper(subInterval.high, domInterval.high, compare) > 0
    ) {
      return false
    }
  }
  return true
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
    const diff = semver.compareIdentifiers(String(lhs[idx]), String(rhs[idx]))
    if (diff !== 0) return diff
  }
  return lhs.length - rhs.length
}
