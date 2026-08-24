export function clampLimit(
	limit: number | undefined,
	defaultLimit: number,
	maxLimit: number,
): number {
	if (!limit || !Number.isFinite(limit) || limit <= 0) return defaultLimit;
	return Math.min(limit, maxLimit);
}
