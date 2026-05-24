export function getSourceControlTools(client) {
    return [
        {
            name: 'n8n_source_control_pull',
            description: 'Pull changes from the source control repository',
            inputSchema: {
                type: 'object',
                properties: {
                    force: { type: 'boolean', description: 'Force pull even if there are conflicts' },
                    variables: { type: 'object', description: 'Variable overrides as key-value pairs' },
                },
            },
            execute: async (args) => {
                const body = {
                    force: args.force,
                    variables: args.variables,
                };
                return client.post('/source-control/pull', body);
            },
        },
    ];
}
