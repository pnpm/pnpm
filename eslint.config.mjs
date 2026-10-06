import eslintConfig, { sizeAndShapeRules } from "@pnpm/eslint-config";
import * as regexpPlugin from "eslint-plugin-regexp";

// Packages whose code still exceeds the size and shape limits. Remove a
// package from this list once it is refactored to pass them.
const PENDING_SIZE_AND_SHAPE_REFACTOR = [
    ".meta-updater/**",
]

export default [
    {
        ignores: ["**/fixtures", "**/__fixtures__", "**/node_modules", "**/lib", ".claude/**", "bench-work-env/**", "target/**", "**/example"],
    },
    ...eslintConfig,
    regexpPlugin.configs['flat/recommended'],
    {
        files: ["pnpm11/pnpm/src/**/*.ts"],
        rules: {
            "import-x/no-extraneous-dependencies": "off",
        },
    },
    ...(PENDING_SIZE_AND_SHAPE_REFACTOR.length > 0 ? [{
        files: PENDING_SIZE_AND_SHAPE_REFACTOR,
        rules: Object.fromEntries(
            Object.keys(sizeAndShapeRules)
                .filter((rule) => rule !== "perfectionist/overly-complex-condition" && rule !== "perfectionist/overly-long-method-chain")
                .map((rule) => [rule, "off"])
        ),
    }] : []),
]
