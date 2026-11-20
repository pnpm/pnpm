import eslintConfig from "@pnpm/eslint-config";
import * as regexpPlugin from "eslint-plugin-regexp";

export default [
    {
        ignores: ["**/fixtures", "**/__fixtures__", "**/node_modules", "**/lib", ".claude/**", "bench-work-env/**", "target/**", "**/example"],
    },
    ...eslintConfig,
    regexpPlugin.configs['flat/recommended'],
    {
        files: ["pnpm/npm/get-pnpm/test/**/*.ts"],
        rules: {
            // The tests run on node:test, whose test() and describe() return a
            // promise that the runner itself awaits.
            "@typescript-eslint/no-floating-promises": ["error", {
                allowForKnownSafeCalls: [
                    { from: "package", package: "node:test", name: ["describe", "it", "suite", "test"] },
                ],
            }],
        },
    },
    {
        files: ["pnpm11/pnpm/src/**/*.ts"],
        rules: {
            "import-x/no-extraneous-dependencies": "off",
        },
    },
]
