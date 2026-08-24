export type AsyncSqlValue = string | number | boolean | null;
export type AsyncSqlRow = Record<string, unknown>;

export interface AsyncSqlRunner {
	query(text: string, params?: readonly AsyncSqlValue[]): Promise<AsyncSqlRow[]>;
}

export interface AsyncSqlDb extends AsyncSqlRunner {
	transaction<T>(fn: (tx: AsyncSqlRunner) => Promise<T>): Promise<T>;
}

export async function queryOne(
	runner: AsyncSqlRunner,
	text: string,
	params?: readonly AsyncSqlValue[],
): Promise<AsyncSqlRow | undefined> {
	const rows = await runner.query(text, params);
	return rows[0];
}
