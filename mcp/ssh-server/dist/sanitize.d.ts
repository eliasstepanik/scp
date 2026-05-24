/**
 * Sanitization utilities — ensure passwords never appear in logs or error messages.
 */
/**
 * Register a value as sensitive. All registered values will be
 * replaced with "***" in any string passed to sanitize().
 */
export declare function registerSensitive(value: string): void;
/**
 * Replace all registered sensitive values in a string with "***".
 * Also redacts common password-like patterns.
 */
export declare function sanitize(text: string): string;
/**
 * Sanitize an Error object's message and return a safe string.
 */
export declare function sanitizeError(err: unknown): string;
//# sourceMappingURL=sanitize.d.ts.map