import type { N8nClient } from '../client.js';
export declare function getAuditTools(client: N8nClient): {
    name: string;
    description: string;
    inputSchema: {
        type: string;
        properties: {
            daysAbandonedWorkflow: {
                type: string;
                description: string;
            };
            categories: {
                type: string;
                items: {
                    type: string;
                };
                description: string;
            };
        };
    };
    execute: (args: Record<string, unknown>) => Promise<unknown>;
}[];
