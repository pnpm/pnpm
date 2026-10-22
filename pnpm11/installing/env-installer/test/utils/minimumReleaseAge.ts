// The mocked registry publishes @pnpm.e2e/bravo-dep 1.0.1 on 2022-02-22 and
// 1.1.0 on 2022-05-01, so this cutoff admits 1.0.1 and not 1.1.0.
export function bravoDepMatureUpTo101MinimumReleaseAge (): number {
  return Math.floor((Date.now() - Date.parse('2022-03-01T00:00:00Z')) / 60_000)
}
