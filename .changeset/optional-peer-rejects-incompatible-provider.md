---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
"pacquet": patch
---

An optional peer dependency is no longer resolved from a version found elsewhere in the workspace when that package's own peer dependencies would conflict with what the project provides. For example, a React 18 project that uses `@react-three/fiber` no longer gets a sibling project's `react-native@0.84.0` as fiber's optional peer, which then reported an unmet `react@^19.2.3` peer [#13989](https://github.com/pnpm/pnpm/issues/13989).
