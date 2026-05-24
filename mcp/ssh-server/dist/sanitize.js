/**
 * Sanitization utilities — ensure passwords never appear in logs or error messages.
 */
/** Registry of sensitive strings to redact */
const sensitiveValues = new Set();
/**
 * Register a value as sensitive. All registered values will be
 * replaced with "***" in any string passed to sanitize().
 */
export function registerSensitive(value) {
    if (value && value.length > 0) {
        sensitiveValues.add(value);
    }
}
/**
 * Replace all registered sensitive values in a string with "***".
 * Also redacts common password-like patterns.
 */
export function sanitize(text) {
    let result = text;
    // Replace registered sensitive values
    for (const secret of sensitiveValues) {
        // Escape special regex characters in the secret
        const escaped = secret.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
        result = result.replace(new RegExp(escaped, 'g'), '***');
    }
    // Redact common password patterns in error messages
    result = result.replace(/--password[=\s]\S+/gi, '--password ***');
    result = result.replace(/--sudoPassword[=\s]\S+/gi, '--sudoPassword ***');
    result = result.replace(/password[=:]\s*\S+/gi, 'password: ***');
    return result;
}
/**
 * Sanitize an Error object's message and return a safe string.
 */
export function sanitizeError(err) {
    if (err instanceof Error) {
        return sanitize(err.message);
    }
    return sanitize(String(err));
}
//# sourceMappingURL=sanitize.js.map