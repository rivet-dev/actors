import type {
	AgentAttemptMarker,
	AgentDispatchAdmission,
	AgentSubmission,
	AgentSubmissionInput,
	AgentSubmissionStore,
	DispatchInput,
	SubmissionAttemptRef,
	SubmissionClaimRef,
	SubmissionDurability,
	SubmissionSettlementObligation,
} from '@flue/runtime/adapter-kit';
import {
	createDispatchAgentSubmissionInput,
	createSessionStorageKey,
} from '@flue/runtime/adapter-kit';
import type { AsyncSqlDb, AsyncSqlRow, AsyncSqlRunner } from './async-db.js';
import {
	DURABILITY_DEFAULT_MAX_ATTEMPTS,
	DURABILITY_DEFAULT_TIMEOUT_MS,
	LEASE_DURATION_MS,
	SUBMISSION_HARNESS_NAME,
	SUBMISSION_SESSION_NAME,
} from './constants.js';

const columns = [
	'sequence', 'submission_id', 'session_key', 'kind', 'payload', 'status', 'accepted_at',
	'canonical_ready_at', 'attempt_id', 'input_applied_at', 'recovery_requested_at',
	'abort_requested_at', 'started_at', 'error', 'attempt_count', 'max_retry', 'timeout_at',
	'owner_id', 'lease_expires_at',
].join(', ');

const prefixedColumns = (table: string) => columns
	.split(', ')
	.map((column) => `${table}.${column}`)
	.join(', ');

export function createAsyncSubmissionStore(db: AsyncSqlDb): AgentSubmissionStore {
	return new AsyncSubmissionStore(db);
}

class AsyncSubmissionStore implements AgentSubmissionStore {
	constructor(private readonly db: AsyncSqlDb) {}

	async getSubmission(submissionId: string): Promise<AgentSubmission | null> {
		const rows = await this.db.query(
			`SELECT ${columns} FROM flue_agent_submissions WHERE submission_id = ? LIMIT 1`,
			[submissionId],
		);
		return rows[0] ? parseSubmission(rows[0]) : null;
	}

	async hasUnsettledSubmissions(): Promise<boolean> {
		return (await this.db.query(
			`SELECT 1 FROM flue_agent_submissions WHERE status IN ('queued', 'running', 'terminalizing') LIMIT 1`,
		)).length > 0;
	}

	async listUnreadySubmissions(): Promise<AgentSubmission[]> {
		return this.readOperational(
			`SELECT ${columns} FROM flue_agent_submissions
			 WHERE status = 'queued' AND canonical_ready_at IS NULL ORDER BY sequence ASC`,
			[],
			'queued',
		);
	}

	async listRunnableSubmissions(): Promise<AgentSubmission[]> {
		return this.readOperational(
			`SELECT ${prefixedColumns('current')} FROM flue_agent_submissions AS current
			 WHERE current.status = 'queued' AND current.canonical_ready_at IS NOT NULL
			   AND NOT EXISTS (
			     SELECT 1 FROM flue_agent_submissions AS earlier
			     WHERE earlier.session_key = current.session_key
			       AND earlier.status IN ('queued', 'running', 'terminalizing')
			       AND earlier.sequence < current.sequence
			   ) ORDER BY current.sequence ASC`,
			[],
			'queued',
		);
	}

	async listRunningSubmissions(): Promise<AgentSubmission[]> {
		return this.readOperational(
			`SELECT ${columns} FROM flue_agent_submissions WHERE status = 'running' ORDER BY sequence ASC`,
			[],
			'active',
		);
	}

	async listPendingSubmissionSettlements(): Promise<SubmissionSettlementObligation[]> {
		const rows = await this.db.query(
			`SELECT submission_id, session_key, attempt_id, settlement_record_id, settlement_record
			 FROM flue_agent_submissions WHERE kind = 'direct' AND status = 'terminalizing'
			 ORDER BY sequence ASC`,
		);
		return rows.map(parseSettlement);
	}

	async replaceSubmissionAttempt(
		attempt: SubmissionAttemptRef,
		nextAttemptId: string,
		lease?: { ownerId: string; leaseExpiresAt: number },
	): Promise<AgentSubmission | null> {
		const rows = await this.db.query(
			`UPDATE flue_agent_submissions
			 SET attempt_id = ?, recovery_requested_at = NULL, started_at = ?,
			     attempt_count = attempt_count + 1${lease ? ', owner_id = ?, lease_expires_at = ?' : ''}
			 WHERE submission_id = ? AND status = 'running' AND attempt_id = ?
			 RETURNING ${columns}`,
			lease
				? [nextAttemptId, Date.now(), lease.ownerId, lease.leaseExpiresAt, attempt.submissionId, attempt.attemptId]
				: [nextAttemptId, Date.now(), attempt.submissionId, attempt.attemptId],
		);
		return rows[0] ? parseSubmission(rows[0]) : null;
	}

	admitDispatch(input: DispatchInput): Promise<AgentDispatchAdmission> {
		return this.admit(createDispatchAgentSubmissionInput(input));
	}

	async admitDirect(input: AgentSubmissionInput): Promise<AgentSubmission> {
		const result = await this.admit(input);
		if (result.kind !== 'submission') {
			throw new Error('[flue] Internal direct admission returned an unexpected result.');
		}
		return result.submission;
	}

	async markSubmissionCanonicalReady(submissionId: string): Promise<AgentSubmission | null> {
		const rows = await this.db.query(
			`UPDATE flue_agent_submissions SET canonical_ready_at = COALESCE(canonical_ready_at, ?)
			 WHERE submission_id = ? AND status = 'queued' RETURNING ${columns}`,
			[Date.now(), submissionId],
		);
		return rows[0] ? parseSubmission(rows[0]) : null;
	}

	async claimSubmission(claim: SubmissionClaimRef): Promise<AgentSubmission | null> {
		const now = Date.now();
		const rows = await this.db.query(
			`UPDATE flue_agent_submissions AS current
			 SET status = 'running', attempt_id = ?, started_at = ?, attempt_count = attempt_count + 1,
			     max_retry = ?, timeout_at = CASE WHEN timeout_at = 0 THEN ? ELSE timeout_at END,
			     owner_id = ?, lease_expires_at = ?
			 WHERE current.submission_id = ? AND current.status = 'queued'
			   AND current.canonical_ready_at IS NOT NULL
			   AND NOT EXISTS (
			     SELECT 1 FROM flue_agent_submissions AS earlier
			     WHERE earlier.session_key = current.session_key
			       AND earlier.status IN ('queued', 'running', 'terminalizing')
			       AND earlier.sequence < current.sequence
			   ) RETURNING ${columns}`,
			[claim.attemptId, now, DURABILITY_DEFAULT_MAX_ATTEMPTS,
				now + DURABILITY_DEFAULT_TIMEOUT_MS, claim.ownerId, claim.leaseExpiresAt, claim.submissionId],
		);
		return rows[0] ? parseSubmission(rows[0]) : null;
	}

	async markSubmissionInputApplied(
		attempt: SubmissionAttemptRef,
		durability?: SubmissionDurability,
	): Promise<boolean> {
		const now = Date.now();
		return (await this.db.query(
			`UPDATE flue_agent_submissions
			 SET input_applied_at = COALESCE(input_applied_at, ?),
			     max_retry = CASE WHEN input_applied_at IS NULL THEN ? ELSE max_retry END,
			     timeout_at = CASE WHEN input_applied_at IS NULL THEN ? ELSE timeout_at END
			 WHERE submission_id = ? AND status = 'running' AND attempt_id = ? RETURNING submission_id`,
			[now, durability?.maxRetry ?? DURABILITY_DEFAULT_MAX_ATTEMPTS,
				durability?.timeoutAt ?? now + DURABILITY_DEFAULT_TIMEOUT_MS,
				attempt.submissionId, attempt.attemptId],
		)).length > 0;
	}

	async requestSubmissionRecovery(attempt: SubmissionAttemptRef): Promise<boolean> {
		return (await this.db.query(
			`UPDATE flue_agent_submissions SET recovery_requested_at = COALESCE(recovery_requested_at, ?)
			 WHERE submission_id = ? AND status = 'running' AND attempt_id = ? RETURNING submission_id`,
			[Date.now(), attempt.submissionId, attempt.attemptId],
		)).length > 0;
	}

	async requestSessionAbort(sessionKey: string): Promise<string[]> {
		const rows = await this.db.query(
			`UPDATE flue_agent_submissions SET abort_requested_at = COALESCE(abort_requested_at, ?)
			 WHERE session_key = ? AND status IN ('queued', 'running') RETURNING submission_id`,
			[Date.now(), sessionKey],
		);
		return rows.map((row) => String(row.submission_id));
	}

	async requeueSubmissionBeforeInputApplied(attempt: SubmissionAttemptRef): Promise<boolean> {
		return (await this.db.query(
			`UPDATE flue_agent_submissions
			 SET status = 'queued', attempt_id = NULL, recovery_requested_at = NULL,
			     started_at = NULL, owner_id = NULL, lease_expires_at = 0
			 WHERE submission_id = ? AND status = 'running' AND attempt_id = ?
			   AND input_applied_at IS NULL RETURNING submission_id`,
			[attempt.submissionId, attempt.attemptId],
		)).length > 0;
	}

	async reserveSubmissionSettlement(
		attempt: SubmissionAttemptRef,
		settlement: { recordId: string; record: SubmissionSettlementObligation['record'] },
	): Promise<SubmissionSettlementObligation | null> {
		if (settlement.record.id !== settlement.recordId) return null;
		const data = JSON.stringify(settlement.record);
		let rows = await this.db.query(
			`UPDATE flue_agent_submissions
			 SET status = 'terminalizing', settlement_record_id = ?, settlement_record = ?
			 WHERE submission_id = ? AND kind = 'direct' AND status = 'running'
			   AND attempt_id = ? AND owner_id IS NOT NULL AND settlement_record_id IS NULL
			 RETURNING submission_id, session_key, attempt_id, settlement_record_id, settlement_record`,
			[settlement.recordId, data, attempt.submissionId, attempt.attemptId],
		);
		if (!rows[0]) rows = await this.db.query(
			`SELECT submission_id, session_key, attempt_id, settlement_record_id, settlement_record
			 FROM flue_agent_submissions WHERE submission_id = ? AND status = 'terminalizing' AND attempt_id = ?`,
			[attempt.submissionId, attempt.attemptId],
		);
		const row = rows[0];
		if (!row || row.settlement_record_id !== settlement.recordId || row.settlement_record !== data) return null;
		return parseSettlement(row);
	}

	async finalizeSubmissionSettlement(attempt: SubmissionAttemptRef, recordId: string): Promise<boolean> {
		return (await this.db.query(
			`UPDATE flue_agent_submissions SET status = 'settled', settled_at = ?
			 WHERE submission_id = ? AND status = 'terminalizing' AND attempt_id = ?
			   AND settlement_record_id = ? RETURNING submission_id`,
			[Date.now(), attempt.submissionId, attempt.attemptId, recordId],
		)).length > 0;
	}

	completeSubmission(attempt: SubmissionAttemptRef): Promise<boolean> {
		return this.settle(attempt, null);
	}

	failSubmission(attempt: SubmissionAttemptRef, error: unknown): Promise<boolean> {
		return this.settle(attempt, error instanceof Error ? error.message : String(error));
	}

	async insertAttemptMarker(attempt: SubmissionAttemptRef): Promise<void> {
		await this.db.query(
			`INSERT OR IGNORE INTO flue_agent_attempt_markers (submission_id, attempt_id, created_at) VALUES (?, ?, ?)`,
			[attempt.submissionId, attempt.attemptId, Date.now()],
		);
	}

	async deleteAttemptMarker(attempt: SubmissionAttemptRef): Promise<void> {
		await this.db.query(
			`DELETE FROM flue_agent_attempt_markers WHERE submission_id = ? AND attempt_id = ?`,
			[attempt.submissionId, attempt.attemptId],
		);
	}

	async listAttemptMarkers(): Promise<AgentAttemptMarker[]> {
		return (await this.db.query(
			`SELECT submission_id, attempt_id, created_at FROM flue_agent_attempt_markers`,
		)).map((row) => ({
			submissionId: String(row.submission_id),
			attemptId: String(row.attempt_id),
			createdAt: Number(row.created_at),
		}));
	}

	async renewLeases(ownerId: string, submissionIds: string[]): Promise<void> {
		if (submissionIds.length === 0) return;
		await this.db.query(
			`UPDATE flue_agent_submissions SET lease_expires_at = ?
			 WHERE owner_id = ? AND status = 'running' AND submission_id IN (${submissionIds.map(() => '?').join(', ')})`,
			[Date.now() + LEASE_DURATION_MS, ownerId, ...submissionIds],
		);
	}

	listExpiredSubmissions(): Promise<AgentSubmission[]> {
		return this.readOperational(
			`SELECT ${columns} FROM flue_agent_submissions
			 WHERE status = 'running' AND lease_expires_at > 0 AND lease_expires_at < ? ORDER BY sequence ASC`,
			[Date.now()],
			'active',
		);
	}

	private async admit(input: AgentSubmissionInput): Promise<AgentDispatchAdmission> {
		const payload = JSON.stringify(input);
		const acceptedAt = Date.parse(input.acceptedAt);
		if (!Number.isFinite(acceptedAt)) throw new Error('[flue] Submission acceptedAt is invalid.');
		const sessionKey = createSessionStorageKey(
			input.id,
			SUBMISSION_HARNESS_NAME,
			SUBMISSION_SESSION_NAME,
		);
		return this.db.transaction(async (tx) => {
			if (input.kind === 'dispatch') {
				const receipts = await tx.query(
					`SELECT dispatch_id, accepted_at FROM flue_agent_dispatch_receipts WHERE dispatch_id = ? LIMIT 1`,
					[input.submissionId],
				);
				if (receipts[0]) return {
					kind: 'retained_receipt' as const,
					receipt: { submissionId: String(receipts[0].dispatch_id), acceptedAt: Number(receipts[0].accepted_at) },
				};
			}
			await tx.query(
				`INSERT OR IGNORE INTO flue_agent_submissions
				 (submission_id, session_key, kind, payload, status, accepted_at)
				 VALUES (?, ?, ?, ?, 'queued', ?)`,
				[input.submissionId, sessionKey, input.kind, payload, acceptedAt],
			);
			const rows = await tx.query(
				`SELECT ${columns} FROM flue_agent_submissions WHERE submission_id = ? LIMIT 1`,
				[input.submissionId],
			);
			const row = rows[0];
			if (!row) throw new Error('[flue] Submission admission did not create a row.');
			if (row.kind !== input.kind || row.payload !== payload) return { kind: 'conflict' as const };
			return { kind: 'submission' as const, submission: parseSubmission(row) };
		});
	}

	private async settle(attempt: SubmissionAttemptRef, error: string | null): Promise<boolean> {
		return (await this.db.query(
			`UPDATE flue_agent_submissions SET status = 'settled', settled_at = ?, error = ?
			 WHERE submission_id = ? AND status = 'running' AND attempt_id = ? RETURNING submission_id`,
			[Date.now(), error, attempt.submissionId, attempt.attemptId],
		)).length > 0;
	}

	private async readOperational(
		text: string,
		params: readonly (string | number | boolean | null)[],
		status: 'queued' | 'active',
	): Promise<AgentSubmission[]> {
		return this.db.transaction(async (tx) => {
			const rows = await tx.query(text, params);
			const output: AgentSubmission[] = [];
			for (const row of rows) {
				try {
					output.push(parseSubmission(row));
				} catch (error) {
					await failMalformed(tx, Number(row.sequence), status, error);
				}
			}
			return output;
		});
	}
}

function parseSubmission(row: AsyncSqlRow): AgentSubmission {
	const input = JSON.parse(String(row.payload)) as AgentSubmissionInput;
	const status = String(row.status) as AgentSubmission['status'];
	if (!['queued', 'running', 'terminalizing', 'settled'].includes(status)) {
		throw new Error('[flue] Persisted submission status is invalid.');
	}
	return {
		sequence: Number(row.sequence),
		submissionId: String(row.submission_id),
		sessionKey: String(row.session_key),
		kind: String(row.kind) as AgentSubmission['kind'],
		input,
		status,
		acceptedAt: Number(row.accepted_at),
		canonicalReadyAt: row.canonical_ready_at == null ? null : Number(row.canonical_ready_at),
		...(row.attempt_id == null ? {} : { attemptId: String(row.attempt_id) }),
		...(row.input_applied_at == null ? {} : { inputAppliedAt: Number(row.input_applied_at) }),
		...(row.recovery_requested_at == null ? {} : { recoveryRequestedAt: Number(row.recovery_requested_at) }),
		...(row.abort_requested_at == null ? {} : { abortRequestedAt: Number(row.abort_requested_at) }),
		...(row.started_at == null ? {} : { startedAt: Number(row.started_at) }),
		...(row.error == null ? {} : { error: String(row.error) }),
		attemptCount: Number(row.attempt_count),
		maxRetry: Number(row.max_retry),
		timeoutAt: Number(row.timeout_at),
		...(row.owner_id == null ? {} : { ownerId: String(row.owner_id) }),
		leaseExpiresAt: Number(row.lease_expires_at),
	};
}

function parseSettlement(row: AsyncSqlRow): SubmissionSettlementObligation {
	return {
		submissionId: String(row.submission_id),
		sessionKey: String(row.session_key),
		attemptId: String(row.attempt_id),
		recordId: String(row.settlement_record_id),
		record: JSON.parse(String(row.settlement_record)) as SubmissionSettlementObligation['record'],
	};
}

async function failMalformed(
	runner: AsyncSqlRunner,
	sequence: number,
	status: 'queued' | 'active',
	error: unknown,
): Promise<void> {
	if (!Number.isFinite(sequence)) throw error;
	await runner.query(
		`UPDATE flue_agent_submissions SET status = 'settled', settled_at = ?, error = ?
		 WHERE sequence = ? AND status = ?`,
		[Date.now(), error instanceof Error ? error.message : String(error), sequence,
			status === 'queued' ? 'queued' : 'running'],
	);
}
