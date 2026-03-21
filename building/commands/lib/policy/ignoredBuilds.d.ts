import type { Config } from '@pnpm/config.reader';
export type IgnoredBuildsCommandOpts = Pick<Config, 'modulesDir' | 'dir' | 'allowBuilds' | 'lockfileDir'>;
export declare const commandNames: string[];
export declare function help(): string;
export declare function cliOptionsTypes(): Record<string, unknown>;
export declare function rcOptionsTypes(): Record<string, unknown>;
export declare function handler(opts: IgnoredBuildsCommandOpts): Promise<string>;
