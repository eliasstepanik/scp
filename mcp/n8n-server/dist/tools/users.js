export function getUserTools(client) {
    return [
        {
            name: 'n8n_list_users',
            description: 'List all users in the n8n instance',
            inputSchema: {
                type: 'object',
                properties: {
                    limit: { type: 'number', description: 'Max results' },
                    cursor: { type: 'string', description: 'Pagination cursor' },
                    includeRole: { type: 'boolean', description: 'Include role information' },
                    projectId: { type: 'string', description: 'Filter by project ID' },
                },
            },
            execute: async (args) => {
                return client.get('/users', args);
            },
        },
        {
            name: 'n8n_create_user',
            description: 'Invite/create a new user',
            inputSchema: {
                type: 'object',
                required: ['email'],
                properties: {
                    email: { type: 'string', description: 'User email address' },
                    role: { type: 'string', description: 'User role (global:admin, global:member)' },
                    firstName: { type: 'string', description: 'First name' },
                    lastName: { type: 'string', description: 'Last name' },
                },
            },
            execute: async (args) => {
                // API expects an array of user objects
                const body = {
                    email: args.email,
                    role: args.role,
                    firstName: args.firstName,
                    lastName: args.lastName,
                };
                return client.post('/users', [body]);
            },
        },
        {
            name: 'n8n_get_user',
            description: 'Get a user by ID or email',
            inputSchema: {
                type: 'object',
                required: ['id'],
                properties: {
                    id: { type: 'string', description: 'User ID or email' },
                    includeRole: { type: 'boolean', description: 'Include role information' },
                },
            },
            execute: async (args) => {
                const { id, ...query } = args;
                return client.get(`/users/${id}`, query);
            },
        },
        {
            name: 'n8n_delete_user',
            description: 'Delete a user by ID',
            inputSchema: {
                type: 'object',
                required: ['id'],
                properties: {
                    id: { type: 'string', description: 'User ID' },
                },
            },
            execute: async (args) => {
                return client.delete(`/users/${args.id}`);
            },
        },
        {
            name: 'n8n_update_user_role',
            description: 'Update a user\'s role',
            inputSchema: {
                type: 'object',
                required: ['id', 'newRoleName'],
                properties: {
                    id: { type: 'string', description: 'User ID' },
                    newRoleName: { type: 'string', description: 'New role name (global:admin, global:member)' },
                },
            },
            execute: async (args) => {
                const body = { newRoleName: args.newRoleName };
                return client.patch(`/users/${args.id}/role`, body);
            },
        },
    ];
}
