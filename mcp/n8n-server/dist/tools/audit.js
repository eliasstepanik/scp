export function getAuditTools(client) {
    return [
        {
            name: 'n8n_generate_audit',
            description: 'Generate a security audit report for the n8n instance',
            inputSchema: {
                type: 'object',
                properties: {
                    daysAbandonedWorkflow: {
                        type: 'number',
                        description: 'Number of days after which a workflow is considered abandoned',
                    },
                    categories: {
                        type: 'array',
                        items: { type: 'string' },
                        description: 'Audit categories to include (e.g. credentials, database, filesystem, nodes, instance)',
                    },
                },
            },
            execute: async (args) => {
                const body = {};
                if (args.daysAbandonedWorkflow !== undefined || args.categories !== undefined) {
                    body.additionalOptions = {};
                    if (args.daysAbandonedWorkflow !== undefined) {
                        body.additionalOptions.daysAbandonedWorkflow = args.daysAbandonedWorkflow;
                    }
                    if (args.categories !== undefined) {
                        body.additionalOptions.categories = args.categories;
                    }
                }
                return client.post('/audit', body);
            },
        },
    ];
}
