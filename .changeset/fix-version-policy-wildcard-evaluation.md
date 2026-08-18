---
"@pnpm/config.version-policy": patch
"pnpm": patch
"pacquet": patch
---

Fixed an inconsistency where `minimumReleaseAgeExclude` (and `trustPolicyExclude`) wildcard/bare-name rules behaved differently in the evaluator and normalizer. A bare rule now consistently evaluates as matching every version, preventing unexpected behavior and silent widening of version policy exemptions when pnpm rewrites the workspace manifest [pnpm/pnpm#13725](https://github.com/pnpm/pnpm/issues/13725).
