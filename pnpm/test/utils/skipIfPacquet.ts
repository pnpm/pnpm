import { describe, test } from '@jest/globals'

import { isPacquetMode } from './execPnpm.js'

/**
 * Use in place of `test()` for cases that fail when the test runs against the
 * pacquet Rust port (selected via the `PNPM_E2E_BIN` env var). Pass-through
 * when running against the bundled pnpm.
 */
export const skipIfPacquet: typeof test = isPacquetMode ? test.skip : test

/** describe()-level variant for skipping whole suites that aren't in pacquet's surface yet. */
export const describeSkipIfPacquet: typeof describe = isPacquetMode ? describe.skip : describe
