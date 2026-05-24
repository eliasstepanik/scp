export function getVariableTools(client) {
    return [
        {
            name: 'n8n_list_variables',
            description: 'List all variables',
            inputSchema: {
                type: 'object',
                properties: {
                    limit: { type: 'number', description: 'Max results' },
                    cursor: { type: 'string', description: 'Pagination cursor' },
                },
            },
            execute: async (args) => {
                return client.get('/variables', args);
            },
        },
        {
            name: 'n8n_create_variable',
            description: 'Create a new variable',
            inputSchema: {
                type: 'object',
                required: ['key', 'value'],
                properties: {
                    key: { type: 'string', description: 'Variable key/name' },
                    value: { type: 'string', description: 'Variable value' },
                    type: { type: 'string', description: 'Variable type (string, number, boolean, etc.)' },
                },
            },
            execute: async (args) => {
                const body = {
                    key: args.key,
                    value: args.value,
                    type: args.type,
                };
                return client.post('/variables', body);
            },
        },
        {
            name: 'n8n_update_variable',
            description: 'Update a variable by ID',
            inputSchema: {
                type: 'object',
                required: ['id'],
                properties: {
                    id: { type: 'string', description: 'Variable ID' },
                    key: { type: 'string', description: 'New key/name' },
                    value: { type: 'string', description: 'New value' },
                    type: { type: 'string', description: 'New type' },
                },
            },
            execute: async (args) => {
                const { id, ...body } = args;
                return client.put(`/variables/${id}`, body);
            },
        },
        {
            name: 'n8n_delete_variable',
            description: 'Delete a variable by ID',
            inputSchema: {
                type: 'object',
                required: ['id'],
                properties: {
                    id: { type: 'string', description: 'Variable ID' },
                },
            },
            execute: async (args) => {
                return client.delete(`/variables/${args.id}`);
            },
        },
    ];
}
