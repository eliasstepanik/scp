import type { N8nConfig } from './types.js';
export declare class N8nClient {
    private baseUrl;
    private apiKey;
    constructor(config: N8nConfig);
    request<T>(method: string, path: string, body?: unknown, queryParams?: Record<string, string | number | boolean | undefined>): Promise<T>;
    get<T>(path: string, queryParams?: Record<string, string | number | boolean | undefined>): Promise<T>;
    post<T>(path: string, body?: unknown): Promise<T>;
    put<T>(path: string, body?: unknown): Promise<T>;
    patch<T>(path: string, body?: unknown): Promise<T>;
    delete<T>(path: string): Promise<T>;
}
