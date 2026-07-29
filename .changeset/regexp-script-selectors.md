---
"pacquet": minor
---

`pnpm run "/^build:(backend|frontend)$/"` selects every script whose name matches the pattern, in single-project and recursive runs alike [#13322](https://github.com/pnpm/pnpm/issues/13322). Flags on the selector are rejected with `ERR_PNPM_UNSUPPORTED_SCRIPT_COMMAND_FORMAT`, as pnpm does.
