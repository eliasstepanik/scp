export function getCredentialTools(client) {
    return [
        {
            name: 'n8n_list_credentials',
            description: 'List all credentials in the n8n instance',
            inputSchema: {
                type: 'object',
                properties: {
                    limit: { type: 'number', description: 'Max results to return' },
                    cursor: { type: 'string', description: 'Pagination cursor' },
                    includeData: { type: 'boolean', description: 'Include credential data' },
                    type: { type: 'string', description: 'Filter by credential type' },
                    name: { type: 'string', description: 'Filter by name' },
                    projectId: { type: 'string', description: 'Filter by project ID' },
                },
            },
            execute: async (args) => {
                return client.get('/credentials', args);
            },
        },
        {
            name: 'n8n_create_credential',
            description: 'Create a new credential',
            inputSchema: {
                type: 'object',
                required: ['name', 'type', 'data'],
                properties: {
                    name: { type: 'string', description: 'Credential name' },
                    type: { type: 'string', description: 'Credential type (e.g. githubApi, slackApi)' },
                    data: { type: 'object', description: 'Credential data (type-specific fields)' },
                    projectId: { type: 'string', description: 'Project to assign credential to' },
                },
            },
            execute: async (args) => {
                const body = {
                    name: args.name,
                    type: args.type,
                    data: args.data,
                    projectId: args.projectId,
                };
                return client.post('/credentials', body);
            },
        },
        {
            name: 'n8n_get_credential',
            description: 'Get a credential by ID',
            inputSchema: {
                type: 'object',
                required: ['id'],
                properties: {
                    id: { type: 'string', description: 'Credential ID' },
                    includeData: { type: 'boolean', description: 'Include credential data' },
                },
            },
            execute: async (args) => {
                const { id, ...query } = args;
                return client.get(`/credentials/${id}`, query);
            },
        },
        {
            name: 'n8n_update_credential',
            description: 'Update a credential by ID',
            inputSchema: {
                type: 'object',
                required: ['id'],
                properties: {
                    id: { type: 'string', description: 'Credential ID' },
                    name: { type: 'string', description: 'New name' },
                    data: { type: 'object', description: 'New credential data' },
                },
            },
            execute: async (args) => {
                const { id, ...body } = args;
                return client.patch(`/credentials/${id}`, body);
            },
        },
        {
            name: 'n8n_delete_credential',
            description: 'Delete a credential by ID',
            inputSchema: {
                type: 'object',
                required: ['id'],
                properties: {
                    id: { type: 'string', description: 'Credential ID' },
                },
            },
            execute: async (args) => {
                return client.delete(`/credentials/${args.id}`);
            },
        },
        {
            name: 'n8n_test_credential',
            description: 'Test a credential by ID',
            inputSchema: {
                type: 'object',
                required: ['id'],
                properties: {
                    id: { type: 'string', description: 'Credential ID' },
                },
            },
            execute: async (args) => {
                return client.post(`/credentials/${args.id}/test`);
            },
        },
        {
            name: 'n8n_get_credential_schema',
            description: 'Get the schema for a credential type',
            inputSchema: {
                type: 'object',
                required: ['credentialTypeName'],
                properties: {
                    credentialTypeName: { type: 'string', description: 'Credential type name (e.g. githubApi)' },
                },
            },
            execute: async (args) => {
                return client.get(`/credentials/schema/${args.credentialTypeName}`);
            },
        },
        {
            name: 'n8n_transfer_credential',
            description: 'Transfer a credential to another project',
            inputSchema: {
                type: 'object',
                required: ['id', 'destinationProjectId'],
                properties: {
                    id: { type: 'string', description: 'Credential ID' },
                    destinationProjectId: { type: 'string', description: 'Target project ID' },
                },
            },
            execute: async (args) => {
                const { id, ...body } = args;
                return client.put(`/credentials/${id}/transfer`, body);
            },
        },
    ];
}
