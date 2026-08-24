import type {
	AttachmentStore,
	PutAttachmentInput,
	StoredAttachment,
} from '@flue/runtime/adapter-kit';
import {
	AttachmentConflictError,
	attachmentBytesEqual,
	sameAttachmentRef,
	verifyAttachmentBytes,
} from '@flue/runtime/adapter-kit';
import type { AsyncSqlDb } from './async-db.js';

export function createAsyncAttachmentStore(db: AsyncSqlDb): AttachmentStore {
	return new AsyncAttachmentStore(db);
}

class AsyncAttachmentStore implements AttachmentStore {
	constructor(private readonly db: AsyncSqlDb) {}

	async put(input: PutAttachmentInput): Promise<void> {
		await verifyAttachmentBytes(input.attachment, input.bytes);
		const data = Buffer.from(input.bytes).toString('base64');
		await this.db.transaction(async (tx) => {
			await tx.query(
				`INSERT OR IGNORE INTO flue_attachments
				 (stream_path, attachment_id, conversation_id, mime_type, filename, size, digest, data)
				 VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
				[input.streamPath, input.attachment.id, input.conversationId, input.attachment.mimeType,
					input.attachment.filename ?? null, input.attachment.size, input.attachment.digest, data],
			);
			const rows = await tx.query(
				`SELECT conversation_id, mime_type, filename, size, digest, data FROM flue_attachments
				 WHERE stream_path = ? AND attachment_id = ? LIMIT 1`,
				[input.streamPath, input.attachment.id],
			);
			const row = rows[0];
			if (!row) throw new Error('[flue] Attachment admission did not create a row.');
			const existing = rowToStored(input.attachment.id, row);
			if (String(row.conversation_id) !== input.conversationId ||
				!sameAttachmentRef(existing.attachment, input.attachment) ||
				!attachmentBytesEqual(existing.bytes, input.bytes)) {
				throw new AttachmentConflictError({
					path: input.streamPath,
					attachmentId: input.attachment.id,
				});
			}
		});
	}

	async get(input: {
		streamPath: string;
		conversationId: string;
		attachmentId: string;
	}): Promise<StoredAttachment | null> {
		const rows = await this.db.query(
			`SELECT conversation_id, mime_type, filename, size, digest, data FROM flue_attachments
			 WHERE stream_path = ? AND attachment_id = ? LIMIT 1`,
			[input.streamPath, input.attachmentId],
		);
		if (!rows[0] || String(rows[0].conversation_id) !== input.conversationId) return null;
		const stored = rowToStored(input.attachmentId, rows[0]);
		await verifyAttachmentBytes(stored.attachment, stored.bytes);
		return stored;
	}

	async deleteForInstance(streamPath: string): Promise<void> {
		await this.db.query(`DELETE FROM flue_attachments WHERE stream_path = ?`, [streamPath]);
	}
}

function rowToStored(id: string, row: Record<string, unknown>): StoredAttachment {
	return {
		attachment: {
			id,
			mimeType: String(row.mime_type),
			size: Number(row.size),
			digest: String(row.digest),
			...(typeof row.filename === 'string' ? { filename: row.filename } : {}),
		},
		bytes: Uint8Array.from(Buffer.from(String(row.data), 'base64')),
	};
}
