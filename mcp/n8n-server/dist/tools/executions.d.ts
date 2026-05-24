import type { N8nClient } from '../client.js';
export declare function getExecutionTools(client: N8nClient): ({
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
            workflowId: {
                type: string;
                description: string;
            };
            status: {
                type: string;
                enum: string[];
                description: string;
            };
            includeData: {
                type: string;
                description: string;
            };
            projectId: {
                type: string;
                description: string;
            };
            id?: undefined;
            loadWorkflowFromDatabase?: undefined;
            tags?: undefined;
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
            id: {
                type: string;
                description: string;
            };
            includeData: {
                type: string;
                description: string;
            };
            limit?: undefined;
            cursor?: undefined;
            workflowId?: undefined;
            status?: undefined;
            projectId?: undefined;
            loadWorkflowFromDatabase?: undefined;
            tags?: undefined;
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
            workflowId?: undefined;
            status?: undefined;
            includeData?: undefined;
            projectId?: undefined;
            loadWorkflowFromDatabase?: undefined;
            tags?: undefined;
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
            loadWorkflowFromDatabase: {
                type: string;
                description: string;
            };
            limit?: undefined;
            cursor?: undefined;
            workflowId?: undefined;
            status?: undefined;
            includeData?: undefined;
            projectId?: undefined;
            tags?: undefined;
        };
    };
    execute: (args: Record<string, unknown>) => Promise<unknown>;
} | {
    name: string;
    description: string;
    inputSchema: {
        type: string;
        properties: {
            limit?: undefined;
            cursor?: undefined;
            workflowId?: undefined;
            status?: undefined;
            includeData?: undefined;
            projectId?: undefined;
            id?: undefined;
            loadWorkflowFromDatabase?: undefined;
            tags?: undefined;
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
            id: {
                type: string;
                description: string;
            };
            tags: {
                type: string;
                items: {
                    type: string;
                    properties: {
                        id: {
                            type: string;
                        };
                    };
                };
                description: string;
            };
            limit?: undefined;
            cursor?: undefined;
            workflowId?: undefined;
            status?: undefined;
            includeData?: undefined;
            projectId?: undefined;
            loadWorkflowFromDatabase?: undefined;
        };
    };
    execute: (args: Record<string, unknown>) => Promise<unknown>;
})[];
