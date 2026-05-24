export function getExecutionTools(client) {
    return [
        {
            name: 'n8n_list_executions',
            description: 'List workflow executions',
            inputSchema: {
                type: 'object',
                properties: {
                    limit: { type: 'number', description: 'Max results' },
                    cursor: { type: 'string', description: 'Pagination cursor' },
                    workflowId: { type: 'string', description: 'Filter by workflow ID' },
                    status: {
                        type: 'string',
                        enum: ['error', 'success', 'waiting', 'running', 'canceled'],
                        description: 'Filter by status',
                    },
                    includeData: { type: 'boolean', description: 'Include execution data' },
                    projectId: { type: 'string', description: 'Filter by project ID' },
                },
            },
            execute: async (args) => {
                return client.get('/executions', args);
            },
        },
        {
            name: 'n8n_get_execution',
            description: 'Get a specific execution by ID',
            inputSchema: {
                type: 'object',
                required: ['id'],
                properties: {
                    id: { type: 'number', description: 'Execution ID' },
                    includeData: { type: 'boolean', description: 'Include execution data' },
                },
            },
            execute: async (args) => {
                const { id, ...query } = args;
                return client.get(`/executions/${id}`, query);
            },
        },
        {
            name: 'n8n_delete_execution',
            description: 'Delete an execution by ID',
            inputSchema: {
                type: 'object',
                required: ['id'],
                properties: {
                    id: { type: 'number', description: 'Execution ID' },
                },
            },
            execute: async (args) => {
                return client.delete(`/executions/${args.id}`);
            },
        },
        {
            name: 'n8n_retry_execution',
            description: 'Retry a failed execution',
            inputSchema: {
                type: 'object',
                required: ['id'],
                properties: {
                    id: { type: 'number', description: 'Execution ID' },
                    loadWorkflowFromDatabase: {
                        type: 'boolean',
                        description: 'Load workflow from database instead of execution data',
                    },
                },
            },
            execute: async (args) => {
                const { id, ...body } = args;
                return client.post(`/executions/${id}/retry`, body);
            },
        },
        {
            name: 'n8n_stop_execution',
            description: 'Stop a running execution by ID',
            inputSchema: {
                type: 'object',
                required: ['id'],
                properties: {
                    id: { type: 'number', description: 'Execution ID' },
                },
            },
            execute: async (args) => {
                return client.post(`/executions/${args.id}/stop`);
            },
        },
        {
            name: 'n8n_stop_all_executions',
            description: 'Stop all running executions',
            inputSchema: {
                type: 'object',
                properties: {},
            },
            execute: async (_args) => {
                return client.post('/executions/stop');
            },
        },
        {
            name: 'n8n_get_execution_tags',
            description: 'Get tags for an execution',
            inputSchema: {
                type: 'object',
                required: ['id'],
                properties: {
                    id: { type: 'number', description: 'Execution ID' },
                },
            },
            execute: async (args) => {
                return client.get(`/executions/${args.id}/tags`);
            },
        },
        {
            name: 'n8n_set_execution_tags',
            description: 'Set tags for an execution',
            inputSchema: {
                type: 'object',
                required: ['id', 'tags'],
                properties: {
                    id: { type: 'number', description: 'Execution ID' },
                    tags: {
                        type: 'array',
                        items: { type: 'object', properties: { id: { type: 'string' } } },
                        description: 'Array of tag objects with id field',
                    },
                },
            },
            execute: async (args) => {
                const { id, tags } = args;
                return client.put(`/executions/${id}/tags`, tags);
            },
        },
    ];
}
