export function getCommunityPackageTools(client) {
    return [
        {
            name: 'n8n_list_community_packages',
            description: 'List installed community packages/nodes',
            inputSchema: {
                type: 'object',
                properties: {},
            },
            execute: async (_args) => {
                return client.get('/community-packages');
            },
        },
        {
            name: 'n8n_install_community_package',
            description: 'Install a community package/node',
            inputSchema: {
                type: 'object',
                required: ['name'],
                properties: {
                    name: { type: 'string', description: 'Package name (npm package name)' },
                },
            },
            execute: async (args) => {
                const body = { name: args.name };
                return client.post('/community-packages', body);
            },
        },
        {
            name: 'n8n_update_community_package',
            description: 'Update a community package to the latest version',
            inputSchema: {
                type: 'object',
                required: ['name'],
                properties: {
                    name: { type: 'string', description: 'Package name' },
                },
            },
            execute: async (args) => {
                const body = { name: args.name };
                return client.patch(`/community-packages/${args.name}`, body);
            },
        },
        {
            name: 'n8n_uninstall_community_package',
            description: 'Uninstall a community package',
            inputSchema: {
                type: 'object',
                required: ['name'],
                properties: {
                    name: { type: 'string', description: 'Package name' },
                },
            },
            execute: async (args) => {
                return client.delete(`/community-packages/${args.name}`);
            },
        },
    ];
}
