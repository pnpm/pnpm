## 1100.2.1

### Patch Changes

- Fixed `trustPolicyExclude` and `minimumReleaseAgeExclude` being ignored when set to a single string instead of a list. The value was read one character at a time, so the exclusion never matched the package it named — and a `*` anywhere in it matched every package, silently switching the policy off.

- Fixed an inconsistency where `minimumReleaseAgeExclude` (and `trustPolicyExclude`) wildcard/bare-name rules behaved differently in the evaluator and normalizer. A bare rule now consistently evaluates as matching every version, preventing unexpected behavior and silent widening of version policy exemptions when pnpm rewrites the workspace manifest [pnpm/pnpm#13725](https://github.com/pnpm/pnpm/issues/13725).

- Updated dependencies:
  - @pnpm/error@1100.1.3
  - @pnpm/types@1102.0.0
