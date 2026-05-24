import type { N8nClient } from '../client.js';
export declare function getCommunityPackageTools(client: N8nClient): ({
    name: string;
    description: string;
    inputSchema: {
        type: string;
        properties: {
            name?: undefined;
        };
        required?: undefined;
    };
    execute: (_args: Record<string, unknown>) => Promise<unknown>;
} | {
    name: string;
    description: string;
    inputSchema: {
        type: string;
        required: string[];
        properties: {
            name: {
                type: string;
                description: string;
            };
        };
    };
    execute: (args: Record<string, unknown>) => Promise<unknown>;
})[];
