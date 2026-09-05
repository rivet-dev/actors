import {
	faChevronLeft,
	faChevronRight,
	faCode,
	faPlay,
	faRefresh,
	faTable,
	faTableCells,
	Icon,
} from "@rivet-gg/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import _ from "lodash";
import { useCallback, useEffect, useMemo, useState } from "react";
import {
	Badge,
	Button,
	Flex,
	Input,
	ScrollArea,
	Textarea,
	WithTooltip,
} from "@/components";
import { ShimmerLine } from "../shimmer-line";
import { formatValue } from "@/lib/format-value";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "../ui/select";
import {
	actorInspectorQueriesKeys,
	type DatabaseColumn,
	type DatabaseExecuteRequest,
	type DatabaseExecuteResult,
	useActorInspector,
} from "./actor-inspector-context";
import {
	DatabaseTable,
	type DatabaseTableCellContext,
	isBlobColumn,
	renderDatabaseCellValue,
} from "./database/database-table";
import type { ActorId } from "./queries";

const PAGE_SIZE = 100;
const DEFAULT_SQL = [
	"SELECT id, value, created_at",
	"FROM test_data",
	"ORDER BY id DESC",
	"LIMIT 25;",
].join("\n");

interface ActorDatabaseProps {
	actorId: ActorId;
}

type DatabaseBrowserRow = Record<string, unknown>;

type EditingCell = {
	rowKey: string;
	columnName: string;
};

type StagedCellEdit = {
	id: string;
	rowKey: string;
	columnName: string;
	primaryKeys: Array<{ name: string; value: unknown }>;
	originalValue: unknown;
	nextValue: unknown;
	draft: string;
};

export function ActorDatabase({ actorId }: ActorDatabaseProps) {
	const [view, setView] = useState<"tables" | "sql">("tables");

	return (
		<div className="flex flex-1 min-h-0 flex-col">
			<div className="flex items-center gap-2 border-b px-2 py-2">
				<Button
					variant={view === "tables" ? "secondary" : "ghost"}
					size="sm"
					onClick={() => setView("tables")}
				>
					<Icon icon={faTable} />
					Tables
				</Button>
				<Button
					variant={view === "sql" ? "secondary" : "ghost"}
					size="sm"
					onClick={() => setView("sql")}
				>
					<Icon icon={faCode} />
					Query
				</Button>
			</div>
			{view === "tables" ? (
				<ActorDatabaseBrowser actorId={actorId} />
			) : (
				<ActorDatabaseSqlShell actorId={actorId} />
			)}
		</div>
	);
}

function ActorDatabaseBrowser({ actorId }: ActorDatabaseProps) {
	const actorInspector = useActorInspector();
	const queryClient = useQueryClient();
	const { data, refetch } = useQuery(
		actorInspector.actorDatabaseQueryOptions(actorId),
	);
	const [table, setTable] = useState<string | undefined>(
		() => data?.tables?.[0]?.table.name,
	);
	const [page, setPage] = useState(0);

	const selectedTable = table || data?.tables?.[0]?.table.name;

	const {
		data: rows,
		refetch: refetchData,
		isLoading,
	} = useQuery({
		...actorInspector.actorDatabaseRowsQueryOptions(
			actorId,
			selectedTable ?? "",
			page,
			PAGE_SIZE,
		),
		enabled: !!selectedTable,
	});

	const currentTable = data?.tables?.find(
		(current) => current.table.name === selectedTable,
	);
	const primaryKeyColumns = useMemo(() => {
		return [...(currentTable?.columns ?? [])]
			.filter((column) => Boolean(column.pk))
			.sort(
				(a, b) =>
					Number(a.pk ?? Number.MAX_SAFE_INTEGER) -
					Number(b.pk ?? Number.MAX_SAFE_INTEGER),
			);
	}, [currentTable]);
	const canEditRows =
		currentTable?.table.type === "table" && primaryKeyColumns.length > 0;
	const visibleRows = useMemo(() => {
		return (rows ?? []).filter(isDatabaseBrowserRow);
	}, [rows]);
	const rowLookup = useMemo(() => {
		const next = new Map<string, DatabaseBrowserRow>();
		for (const row of visibleRows) {
			const rowKey = createRowKey(row, primaryKeyColumns);
			if (rowKey) {
				next.set(rowKey, row);
			}
		}
		return next;
	}, [primaryKeyColumns, visibleRows]);
	const columnLookup = useMemo(() => {
		return new Map(
			(currentTable?.columns ?? []).map((column) => {
				return [column.name, column] as const;
			}),
		);
	}, [currentTable?.columns]);
	const [editingCell, setEditingCell] = useState<EditingCell | null>(null);
	const [editingValue, setEditingValue] = useState("");
	const [stagedEdits, setStagedEdits] = useState<
		Record<string, StagedCellEdit>
	>({});
	const [isApplyingEdits, setIsApplyingEdits] = useState(false);
	const [tableEditError, setTableEditError] = useState<string | null>(null);
	const { mutateAsync: executeDatabaseSql } = useMutation(
		actorInspector.actorDatabaseExecuteMutation(actorId),
	);
	const stagedEditList = useMemo(() => {
		return Object.values(stagedEdits);
	}, [stagedEdits]);
	const stagedEditCount = stagedEditList.length;

	useEffect(() => {
		setEditingCell(null);
		setEditingValue("");
		setStagedEdits({});
		setTableEditError(null);
	}, []);

	const totalRows = currentTable?.records ?? 0;
	const totalPages = Math.max(1, Math.ceil(totalRows / PAGE_SIZE));
	const hasNextPage = page < totalPages - 1;
	const hasPrevPage = page > 0;

	const beginCellEdit = useCallback(
		({ column, row, value }: DatabaseTableCellContext) => {
			if (!canEditRows || isBlobColumn(column, value)) {
				return;
			}

			const rowKey = createRowKey(row, primaryKeyColumns);
			if (!rowKey) {
				return;
			}

			const editId = createStagedEditId(rowKey, column.name);
			setEditingCell({ rowKey, columnName: column.name });
			setEditingValue(
				stagedEdits[editId]?.draft ?? formatCellDraft(value),
			);
			setTableEditError(null);
		},
		[canEditRows, primaryKeyColumns, stagedEdits],
	);

	const commitCellEdit = useCallback(() => {
		if (!editingCell) {
			return;
		}

		const row = rowLookup.get(editingCell.rowKey);
		const column = columnLookup.get(editingCell.columnName);
		if (!row || !column) {
			setEditingCell(null);
			setEditingValue("");
			return;
		}

		const nextValue = parseEditedCellValue(
			editingValue,
			row[column.name],
			column,
		);
		const editId = createStagedEditId(editingCell.rowKey, column.name);
		const primaryKeys = extractPrimaryKeyValues(row, primaryKeyColumns);
		if (primaryKeys.length !== primaryKeyColumns.length) {
			setEditingCell(null);
			setEditingValue("");
			return;
		}

		setStagedEdits((current) => {
			if (areDatabaseValuesEqual(nextValue, row[column.name])) {
				if (!(editId in current)) {
					return current;
				}
				const next = { ...current };
				delete next[editId];
				return next;
			}

			return {
				...current,
				[editId]: {
					id: editId,
					rowKey: editingCell.rowKey,
					columnName: column.name,
					primaryKeys,
					originalValue: row[column.name],
					nextValue,
					draft: editingValue,
				},
			};
		});
		setEditingCell(null);
		setEditingValue("");
	}, [columnLookup, editingCell, editingValue, primaryKeyColumns, rowLookup]);

	const discardEdits = useCallback(() => {
		setEditingCell(null);
		setEditingValue("");
		setStagedEdits({});
		setTableEditError(null);
	}, []);

	const applyEdits = useCallback(async () => {
		if (!selectedTable || stagedEditList.length === 0) {
			return;
		}

		setIsApplyingEdits(true);
		setTableEditError(null);
		try {
			for (const edit of stagedEditList) {
				const args: unknown[] = [edit.nextValue];
				const whereClauses = edit.primaryKeys.map((primaryKey) => {
					const columnName = quoteSqlIdentifier(primaryKey.name);
					if (primaryKey.value === null) {
						return `"${columnName}" IS NULL`;
					}
					args.push(primaryKey.value);
					return `"${columnName}" = ?`;
				});
				await executeDatabaseSql({
					sql: `UPDATE "${quoteSqlIdentifier(selectedTable)}" SET "${quoteSqlIdentifier(edit.columnName)}" = ? WHERE ${whereClauses.join(" AND ")}`,
					args,
				});
			}

			setEditingCell(null);
			setEditingValue("");
			setStagedEdits({});
			await Promise.all([
				refetch(),
				refetchData(),
				queryClient.invalidateQueries({
					queryKey: actorInspectorQueriesKeys.actorDatabase(actorId),
				}),
			]);
		} catch (error) {
			setTableEditError(
				error instanceof Error
					? error.message
					: "Failed to update edited cells.",
			);
		} finally {
			setIsApplyingEdits(false);
		}
	}, [
		actorId,
		executeDatabaseSql,
		queryClient,
		refetch,
		refetchData,
		selectedTable,
		stagedEditList,
	]);

	const renderBrowserCell = useCallback(
		(context: DatabaseTableCellContext) => {
			const rowKey = createRowKey(context.row, primaryKeyColumns);
			if (!rowKey) {
				return renderDatabaseCellValue(context.column, context.value);
			}

			const editId = createStagedEditId(rowKey, context.column.name);
			const stagedEdit = stagedEdits[editId];
			if (
				editingCell?.rowKey === rowKey &&
				editingCell.columnName === context.column.name
			) {
				return (
					<Input
						autoFocus
						value={editingValue}
						onChange={(event) =>
							setEditingValue(event.target.value)
						}
						onBlur={commitCellEdit}
						onKeyDown={(event) => {
							if (event.key === "Enter") {
								event.preventDefault();
								commitCellEdit();
							} else if (event.key === "Escape") {
								event.preventDefault();
								setEditingCell(null);
								setEditingValue("");
							}
						}}
						className="h-8 font-mono-console text-xs"
					/>
				);
			}

			return renderDatabaseCellValue(
				context.column,
				stagedEdit ? stagedEdit.nextValue : context.value,
			);
		},
		[
			commitCellEdit,
			editingCell,
			editingValue,
			primaryKeyColumns,
			stagedEdits,
		],
	);

	const getBrowserCellClassName = useCallback(
		(context: DatabaseTableCellContext) => {
			const rowKey = createRowKey(context.row, primaryKeyColumns);
			if (!rowKey) {
				return undefined;
			}
			const editId = createStagedEditId(rowKey, context.column.name);
			if (stagedEdits[editId]) {
				return "bg-primary/10 ring-1 ring-inset ring-primary/35";
			}
			if (
				editingCell?.rowKey === rowKey &&
				editingCell.columnName === context.column.name
			) {
				return "bg-primary/15 ring-1 ring-inset ring-primary/45";
			}
			if (canEditRows && !isBlobColumn(context.column, context.value)) {
				return "cursor-text";
			}
			return undefined;
		},
		[canEditRows, editingCell, primaryKeyColumns, stagedEdits],
	);

	return (
		<>
			<div className="flex justify-between items-center border-b gap-1 h-[45px]">
				<div className="border-r h-full">
					<TableSelect
						actorId={actorId}
						onSelect={(nextTable) => {
							setTable(nextTable);
							setPage(0);
						}}
						value={selectedTable}
					/>
				</div>
				<div className="flex-1 text-xs">
					<Flex className="items-center gap-2 h-full px-2">
						<Icon icon={faTableCells} />
						{currentTable ? (
							<>
								{currentTable.table.schema}.
								{currentTable.table.name}
								<span className="text-muted-foreground">
									({currentTable.columns.length} columns,{" "}
									{currentTable.records} rows)
								</span>
							</>
						) : (
							<span className="text-muted-foreground">
								No table selected
							</span>
						)}
					</Flex>
				</div>
				<div className="border-l h-full flex items-center gap-2 px-2">
					{stagedEditCount > 0 ? (
						<>
							<Button
								variant="ghost"
								size="sm"
								disabled={isApplyingEdits}
								onClick={discardEdits}
							>
								Discard
							</Button>
							<Button
								size="sm"
								isLoading={isApplyingEdits}
								onClick={() => {
									void applyEdits();
								}}
							>
								Update {stagedEditCount} Cell
								{stagedEditCount === 1 ? "" : "s"}
							</Button>
						</>
					) : null}
					<div className="flex items-center gap-1">
						<WithTooltip
							content="Previous page"
							trigger={
								<Button
									variant="ghost"
									size="icon-sm"
									disabled={!hasPrevPage}
									onClick={() =>
										setPage((current) => current - 1)
									}
								>
									<Icon icon={faChevronLeft} />
								</Button>
							}
						/>
						<span className="text-xs text-muted-foreground tabular-nums">
							{page + 1} / {totalPages}
						</span>
						<WithTooltip
							content="Next page"
							trigger={
								<Button
									variant="ghost"
									size="icon-sm"
									disabled={!hasNextPage}
									onClick={() =>
										setPage((current) => current + 1)
									}
								>
									<Icon icon={faChevronRight} />
								</Button>
							}
						/>
					</div>
					<WithTooltip
						content="Refresh"
						trigger={
							<Button
								variant="ghost"
								size="icon-sm"
								isLoading={isLoading}
								onClick={() => {
									refetch();
									refetchData();
								}}
							>
								<Icon icon={faRefresh} />
							</Button>
						}
					/>
				</div>
			</div>
			<div className="flex-1 min-h-0 overflow-hidden flex relative">
				{isLoading ? <ShimmerLine /> : null}
				<ScrollArea className="w-full h-full min-h-0">
					{tableEditError ? (
						<div className="border-b px-3 py-2 text-xs text-destructive">
							{tableEditError}
						</div>
					) : null}
					{currentTable ? (
						<DatabaseTable
							className="overflow-hidden"
							columns={currentTable.columns}
							enableColumnResizing={false}
							enableRowSelection={false}
							data={visibleRows}
							references={currentTable.foreignKeys}
							renderCell={renderBrowserCell}
							getCellClassName={getBrowserCellClassName}
							onCellDoubleClick={beginCellEdit}
						/>
					) : null}
				</ScrollArea>
			</div>
		</>
	);
}

function ActorDatabaseSqlShell({ actorId }: ActorDatabaseProps) {
	const actorInspector = useActorInspector();
	const queryClient = useQueryClient();
	const [sql, setSql] = useState(DEFAULT_SQL);
	const [propertyDrafts, setPropertyDrafts] = useState<
		Record<string, string>
	>({});
	const [bindingChangeToken, setBindingChangeToken] = useState(0);
	const [result, setResult] = useState<DatabaseExecuteResult | null>(null);
	const namedBindings = useMemo(() => {
		return extractNamedBindings(sql);
	}, [sql]);
	const hasNamedBindings = namedBindings.length > 0;
	const hasPositionalBindings = useMemo(() => {
		return sql.includes("?");
	}, [sql]);
	const hasMixedBindings = hasNamedBindings && hasPositionalBindings;

	useEffect(() => {
		setPropertyDrafts((current) => {
			const next: Record<string, string> = {};
			for (const name of namedBindings) {
				next[name] = current[name] ?? "";
			}
			return next;
		});
	}, [namedBindings]);

	const parsedProperties = useMemo(() => {
		const next: Record<string, unknown> = {};
		for (const name of namedBindings) {
			const parsed = parseBindingDraft(propertyDrafts[name] ?? "");
			if (parsed.error) {
				return {
					value: {} as Record<string, unknown>,
					error: `${name}: ${parsed.error}`,
				};
			}
			next[name] = parsed.value;
		}
		return {
			value: next,
			error: null,
		};
	}, [namedBindings, propertyDrafts]);

	const resultColumns = useMemo(() => {
		return createResultColumns(result?.rows ?? []);
	}, [result]);
	const resultRowsAreTable = useMemo(() => {
		return areObjectRows(result?.rows ?? []);
	}, [result]);

	const { mutateAsync, isPending, error } = useMutation(
		actorInspector.actorDatabaseExecuteMutation(actorId),
	);
	const bindingError = hasMixedBindings
		? "Mixing positional `?` bindings and named properties is not supported in Inspector. Use one binding style."
		: hasPositionalBindings
			? "Positional `?` bindings are only supported in the Inspector HTTP API. Use named properties in the UI."
			: null;
	const propertiesError = hasNamedBindings ? parsedProperties.error : null;
	const runSql = useCallback(async () => {
		const request: DatabaseExecuteRequest = {
			sql,
		};

		if (hasNamedBindings) {
			request.properties = parsedProperties.value;
		}

		const nextResult = await mutateAsync(request);
		setResult(nextResult);
		await queryClient.invalidateQueries({
			queryKey: actorInspectorQueriesKeys.actorDatabase(actorId),
		});
	}, [
		actorId,
		hasNamedBindings,
		mutateAsync,
		parsedProperties.value,
		queryClient,
		sql,
	]);
	const canRun =
		sql.trim() !== "" && bindingError === null && propertiesError === null;

	useEffect(() => {
		if (
			bindingChangeToken === 0 ||
			!hasNamedBindings ||
			result === null ||
			propertiesError !== null ||
			isPending
		) {
			return;
		}

		const timer = window.setTimeout(() => {
			setBindingChangeToken(0);
			void runSql();
		}, 250);
		return () => window.clearTimeout(timer);
	}, [
		bindingChangeToken,
		hasNamedBindings,
		isPending,
		propertiesError,
		result,
		runSql,
	]);

	return (
		<div className="flex flex-1 min-h-0 flex-col">
			<div className="border-b p-3">
				<div className="flex items-center justify-between gap-3">
					<div>
						<div className="text-sm font-medium">Manual SQL</div>
						<div className="text-xs text-muted-foreground">
							Run statements directly against this actor&apos;s
							SQLite database. Use `RETURNING` when you want
							mutation output.
						</div>
					</div>
					<Button
						size="sm"
						isLoading={isPending}
						disabled={!canRun}
						onClick={() => {
							void runSql();
						}}
					>
						<Icon icon={faPlay} />
						Run
					</Button>
				</div>
				<div className="mt-3">
					<Textarea
						value={sql}
						onChange={(event) => setSql(event.target.value)}
						onKeyDown={async (event) => {
							if (
								(event.metaKey || event.ctrlKey) &&
								event.key === "Enter" &&
								canRun &&
								!isPending
							) {
								event.preventDefault();
								await runSql();
							}
						}}
						className="min-h-36 font-mono-console text-xs"
					/>
				</div>
				{hasNamedBindings ? (
					<div className="mt-3 space-y-2">
						<div className="flex items-center gap-2 text-xs text-muted-foreground">
							<Badge variant="outline">Named properties</Badge>
							<span>
								Edit a property to re-run the query preview.
							</span>
						</div>
						<div className="grid gap-2 md:grid-cols-2">
							{namedBindings.map((name) => (
								<label
									key={name}
									className="flex flex-col gap-1 text-xs"
								>
									<span className="font-mono-console text-muted-foreground">
										{name}
									</span>
									<Input
										value={propertyDrafts[name] ?? ""}
										onChange={(event) => {
											const nextValue =
												event.target.value;
											setPropertyDrafts((current) => ({
												...current,
												[name]: nextValue,
											}));
											if (result !== null) {
												setBindingChangeToken(
													(current) => current + 1,
												);
											}
										}}
										className="font-mono-console text-xs"
										placeholder="value"
									/>
								</label>
							))}
						</div>
					</div>
				) : null}
				{bindingError ? (
					<div className="mt-2 text-xs text-destructive">
						{bindingError}
					</div>
				) : null}
				{propertiesError ? (
					<div className="mt-2 text-xs text-destructive">
						{propertiesError}
					</div>
				) : null}
				{error ? (
					<div className="mt-2 text-xs text-destructive">
						{error instanceof Error
							? error.message
							: "Failed to execute SQL."}
					</div>
				) : null}
			</div>
			<div className="flex-1 min-h-0 overflow-hidden relative">
				{isPending ? <ShimmerLine /> : null}
				<div className="flex items-center justify-between gap-2 border-b px-3 py-2">
					<div className="text-xs text-muted-foreground">
						{result ? (
							<>
								Returned{" "}
								<span className="tabular-nums text-foreground">
									{result.rows.length}
								</span>{" "}
								row{result.rows.length === 1 ? "" : "s"}
							</>
						) : null}
					</div>
					{result ? (
						<Button
							variant="ghost"
							size="sm"
							onClick={() => setResult(null)}
						>
							Clear output
						</Button>
					) : null}
				</div>
				<ScrollArea className="h-full w-full">
					{result === null ? null : result.rows.length === 0 ? (
						<div className="px-3 py-4 text-sm text-muted-foreground">
							Statement executed successfully. No rows were
							returned.
						</div>
					) : resultRowsAreTable && resultColumns.length > 0 ? (
						<DatabaseTable
							className="overflow-hidden"
							columns={resultColumns}
							enableColumnResizing={false}
							enableRowSelection={false}
							enableSorting={false}
							data={result.rows}
						/>
					) : (
						<pre className="overflow-auto px-3 py-4 text-xs">
							{formatValue(result.rows, true)}
						</pre>
					)}
				</ScrollArea>
			</div>
		</div>
	);
}

function TableSelect({
	actorId,
	value,
	onSelect,
}: {
	actorId: ActorId;
	onSelect: (table: string) => void;
	value: string | undefined;
}) {
	const actorInspector = useActorInspector();
	const { data: tables } = useQuery(
		actorInspector.actorDatabaseTablesQueryOptions(actorId),
	);

	return (
		<Select onValueChange={onSelect} value={value}>
			<SelectTrigger
				variant="ghost"
				className="h-full pr-2 rounded-none [&>svg]:!size-3"
			>
				<SelectValue placeholder="Select table or view..." />
			</SelectTrigger>
			<SelectContent>
				{tables?.length === 0 ? (
					<SelectItem disabled value="empty">
						<Flex className="items-center gap-2">
							<Icon icon={faTable} className="text-foreground" />
							No tables found
						</Flex>
					</SelectItem>
				) : null}
				{tables?.map((table) => (
					<SelectItem key={table.name} value={table.name}>
						<div className="flex items-center gap-2">
							<Icon icon={faTable} className="text-foreground" />
							{table.name}
						</div>
					</SelectItem>
				))}
			</SelectContent>
		</Select>
	);
}

function areObjectRows(rows: unknown[]): rows is Record<string, unknown>[] {
	return rows.every(
		(row) => row !== null && typeof row === "object" && !Array.isArray(row),
	);
}

function createResultColumns(rows: unknown[]): DatabaseColumn[] {
	if (!areObjectRows(rows)) {
		return [];
	}

	const names = Array.from(new Set(rows.flatMap((row) => Object.keys(row))));
	return names.map((name, cid) => ({
		cid,
		name,
		type: "",
		notnull: false,
		dflt_value: null,
		pk: false,
	}));
}

function isDatabaseBrowserRow(value: unknown): value is DatabaseBrowserRow {
	return value !== null && typeof value === "object" && !Array.isArray(value);
}

function createRowKey(
	row: DatabaseBrowserRow,
	primaryKeyColumns: DatabaseColumn[],
): string | null {
	if (primaryKeyColumns.length === 0) {
		return null;
	}

	const values = primaryKeyColumns.map((column) => {
		if (!(column.name in row)) {
			return undefined;
		}
		return [column.name, row[column.name]];
	});
	if (values.some((value) => value === undefined)) {
		return null;
	}
	return formatValue(values);
}

function createStagedEditId(rowKey: string, columnName: string): string {
	return `${rowKey}:${columnName}`;
}

function extractPrimaryKeyValues(
	row: DatabaseBrowserRow,
	primaryKeyColumns: DatabaseColumn[],
) {
	return primaryKeyColumns.flatMap((column) => {
		if (!(column.name in row)) {
			return [];
		}
		return [{ name: column.name, value: row[column.name] }];
	});
}

function formatCellDraft(value: unknown): string {
	if (value === null) {
		return "";
	}
	if (typeof value === "string") {
		return value;
	}
	return String(value);
}

function parseEditedCellValue(
	draft: string,
	originalValue: unknown,
	column: DatabaseColumn,
): unknown {
	const trimmed = draft.trim();
	if (trimmed.toLowerCase() === "null") {
		return null;
	}

	const type = column.type.toLowerCase();
	if (
		typeof originalValue === "number" ||
		/\b(int|real|floa|doub|dec|num|bool)\b/.test(type)
	) {
		const numeric = Number(trimmed);
		if (trimmed !== "" && !Number.isNaN(numeric)) {
			return numeric;
		}
	}

	return draft;
}

function areDatabaseValuesEqual(a: unknown, b: unknown): boolean {
	if (Object.is(a, b)) {
		return true;
	}
	return _.isEqual(a, b);
}

function quoteSqlIdentifier(value: string): string {
	return value.replace(/"/g, '""');
}

function extractNamedBindings(sql: string): string[] {
	const matches = sql.matchAll(/([:@$])([A-Za-z_][A-Za-z0-9_]*)/g);
	return Array.from(new Set(Array.from(matches, (match) => match[2])));
}

function parseBindingDraft(value: string): {
	value: unknown;
	error: string | null;
} {
	if (value.trim() === "") {
		return { value: "", error: null };
	}

	try {
		const parsed = JSON.parse(value);
		if (!isSupportedBindingValue(parsed)) {
			return {
				value: "",
				error: "SQLite bindings must be null, number, string, or arrays of numbers.",
			};
		}
		return { value: parsed, error: null };
	} catch {
		return { value, error: null };
	}
}

function isSupportedBindingValue(value: unknown): boolean {
	if (
		value === null ||
		typeof value === "number" ||
		typeof value === "string"
	) {
		return true;
	}

	if (Array.isArray(value)) {
		return value.every((item) => typeof item === "number");
	}

	return false;
}
