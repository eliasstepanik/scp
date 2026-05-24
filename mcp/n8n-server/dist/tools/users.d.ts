import type { N8nClient } from '../client.js';
export declare function getUserTools(client: N8nClient): ({
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
            includeRole: {
                type: string;
                description: string;
            };
            projectId: {
                type: string;
                description: string;
            };
            email?: undefined;
            role?: undefined;
            firstName?: undefined;
            lastName?: undefined;
            id?: undefined;
            newRoleName?: undefined;
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
            email: {
                type: string;
                description: string;
            };
            role: {
                type: string;
                description: string;
            };
            firstName: {
                type: string;
                description: string;
            };
            lastName: {
                type: string;
                description: string;
            };
            limit?: undefined;
            cursor?: undefined;
            includeRole?: undefined;
            projectId?: undefined;
            id?: undefined;
            newRoleName?: undefined;
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
            id: {
                type: string;
                description: string;
            };
            includeRole: {
                type: string;
                description: string;
            };
            limit?: undefined;
            cursor?: undefined;
            projectId?: undefined;
            email?: undefined;
            role?: undefined;
            firstName?: undefined;
            lastName?: undefined;
            newRoleName?: undefined;
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
            id: {
                type: string;
                description: string;
            };
            limit?: undefined;
            cursor?: undefined;
            includeRole?: undefined;
            projectId?: undefined;
            email?: undefined;
            role?: undefined;
            firstName?: undefined;
            lastName?: undefined;
            newRoleName?: undefined;
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
            id: {
                type: string;
                description: string;
            };
            newRoleName: {
                type: string;
                description: string;
            };
            limit?: undefined;
            cursor?: undefined;
            includeRole?: undefined;
            projectId?: undefined;
            email?: undefined;
            role?: undefined;
            firstName?: undefined;
            lastName?: undefined;
        };
    };
    execute: (args: Record<string, unknown>) => Promise<unknown>;
})[];
