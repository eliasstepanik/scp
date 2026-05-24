export class N8nClient {
    baseUrl;
    apiKey;
    constructor(config) {
        this.baseUrl = config.baseUrl.replace(/\/$/, '');
        this.apiKey = config.apiKey;
    }
    async request(method, path, body, queryParams) {
        let url = `${this.baseUrl}/api/v1${path}`;
        if (queryParams) {
            const params = new URLSearchParams();
            for (const [key, value] of Object.entries(queryParams)) {
                if (value !== undefined && value !== null) {
                    params.set(key, String(value));
                }
            }
            const qs = params.toString();
            if (qs)
                url += `?${qs}`;
        }
        const headers = {
            'X-N8N-API-KEY': this.apiKey,
            'Content-Type': 'application/json',
        };
        const init = {
            method,
            headers,
        };
        if (body !== undefined && method !== 'GET' && method !== 'DELETE') {
            init.body = JSON.stringify(body);
        }
        const response = await fetch(url, init);
        if (!response.ok) {
            let errorText;
            try {
                const errJson = await response.json();
                errorText = errJson.message ?? JSON.stringify(errJson);
            }
            catch {
                errorText = await response.text();
            }
            throw new Error(`n8n API error ${response.status}: ${errorText}`);
        }
        // 204 No Content
        if (response.status === 204) {
            return undefined;
        }
        return response.json();
    }
    // Convenience methods
    get(path, queryParams) {
        return this.request('GET', path, undefined, queryParams);
    }
    post(path, body) {
        return this.request('POST', path, body);
    }
    put(path, body) {
        return this.request('PUT', path, body);
    }
    patch(path, body) {
        return this.request('PATCH', path, body);
    }
    delete(path) {
        return this.request('DELETE', path);
    }
}
