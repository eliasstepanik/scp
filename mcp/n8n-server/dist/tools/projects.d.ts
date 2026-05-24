import type { N8nClient } from '../client.js';
export declare function getProjectTools(client: N8nClient): ({
    name: string;
    description: string;
    inputSchema: {
        type: string;
        properties: {
            limit: {
                type: string;
                description: string;
            };
            cursor: {
                type: string;
                description: string;
            };
            name?: undefined;
            type?: undefined;
            projectId?: undefined;
            userId?: undefined;
            role?: undefined;
        };
        required?: undefined;
    };
    execute: (args: Record<string, unknown>) => Promise<unknown>;
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
            type: {
                type: string;
                description: string;
            };
            limit?: undefined;
            cursor?: undefined;
            projectId?: undefined;
            userId?: undefined;
            role?: undefined;
        };
    };
    execute: (args: Record<string, unknown>) => Promise<unknown>;
} | {
    name: string;
    description: string;
    inputSchema: {
        type: string;
        required: string[];
        properties: {
            projectId: {
                type: string;
                description: string;
            };
            limit?: undefined;
            cursor?: undefined;
            name?: undefined;
            type?: undefined;
            userId?: undefined;
            role?: undefined;
        };
    };
    execute: (args: Record<string, unknown>) => Promise<unknown>;
} | {
    name: string;
    description: string;
    inputSchema: {
        type: string;
        required: string[];
        properties: {
            projectId: {
                type: string;
                description: string;
            };
            name: {
                type: string;
                description: string;
            };
            limit?: undefined;
            cursor?: undefined;
            type?: undefined;
            userId?: undefined;
            role?: undefined;
        };
    };
    execute: (args: Record<string, unknown>) => Promise<unknown>;
} | {
    name: string;
    description: string;
    inputSchema: {
        type: string;
        required: string[];
        properties: {
            projectId: {
                type: string;
                description: string;
            };
            limit: {
                type: string;
                description: string;
            };
            cursor: {
                type: string;
                description: string;
            };
            name?: undefined;
            type?: undefined;
            userId?: undefined;
            role?: undefined;
        };
    };
    execute: (args: Record<string, unknown>) => Promise<unknown>;
} | {
    name: string;
    description: string;
    inputSchema: {
        type: string;
        required: string[];
        properties: {
            projectId: {
                type: string;
                description: string;
            };
            userId: {
                type: string;
                description: string;
            };
            role: {
                type: string;
                description: string;
            };
            limit?: undefined;
            cursor?: undefined;
            name?: undefined;
            type?: undefined;
        };
    };
    execute: (args: Record<string, unknown>) => Promise<unknown>;
} | {
    name: string;
    description: string;
    inputSchema: {
        type: string;
        required: string[];
        properties: {
            projectId: {
                type: string;
                description: string;
            };
            userId: {
                type: string;
                description: string;
            };
            limit?: undefined;
            cursor?: undefined;
            name?: undefined;
            type?: undefined;
            role?: undefined;
        };
    };
    execute: (args: Record<string, unknown>) => Promise<unknown>;
})[];
