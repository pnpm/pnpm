# pnpm Benchmarks

Compares `pnpm install` performance between the current branch and `main`.

## Prerequisites

- [hyperfine](https://github.com/sharkdp/hyperfine) — install via `brew install hyperfine`
- The current branch must be compiled (`pnpm run compile`)
- If providing a pre-existing main checkout path, it must also be compiled

## Usage

```sh
pnpm run compile
./benchmarks/bench.sh
```

If a git worktree with `main` already exists, the script finds and uses it automatically. Otherwise it creates one at `../.pnpm-bench-main` (a sibling of the repo), installs dependencies, and compiles.

You can also point to a specific checkout of main:

```sh
./benchmarks/bench.sh /path/to/main
```

## Scenarios

| # | Name | Lockfile | Store + Cache | Description |
|---|---|---|---|---|
| 1 | Headless | ✔ frozen | warm | Repeat install with warm store |
| 2 | Re-resolution | ✔ + add dep | warm | Add a new dependency to an existing lockfile |
| 3 | Full resolution | ✗ | warm | Resolve everything from scratch with warm store and cache |
| 4 | Headless cold | ✔ frozen | cold | Typical CI install — fetch all packages with lockfile |
| 5 | Cold install | ✗ | cold | True cold start — nothing cached |

All scenarios use `--ignore-scripts` and isolated store/cache directories per variant.

## Output

Results are printed to the terminal and saved as:

- `results.md` — consolidated markdown table
- `<scenario>-main.json` / `<scenario>-branch.json` — raw hyperfine data

All files are written to a temp directory printed at the end of the run.

## Configuration

Edit the variables at the top of `bench.sh`:

- `WARMUP` — number of warmup runs before timing (default: 1)
- `RUNS` — number of timed runs per benchmark (default: 10)
