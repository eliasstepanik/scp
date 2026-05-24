import type { N8nClient } from '../client.js';
export declare function getSourceControlTools(client: N8nClient): {
    name: string;
    description: string;
    inputSchema: {
        type: string;
        properties: {
            force: {
                type: string;
                description: string;
            };
            variables: {
                type: string;
                description: string;
            };
        };
    };
    execute: (args: Record<string, unknown>) => Promise<unknown>;
}[];
