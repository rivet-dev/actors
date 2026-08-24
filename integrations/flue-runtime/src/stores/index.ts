import type { PersistenceStores } from '@flue/runtime/adapter-kit';
import type { AsyncSqlDb } from './async-db.js';
import { createAsyncAttachmentStore } from './attachment-store.js';
import { createAsyncConversationStreamStore } from './conversation-stream-store.js';
import { createAsyncEventStreamStore } from './event-stream-store.js';
import { createAsyncRunStore } from './run-store.js';
import { createAsyncSubmissionStore } from './submission-store.js';

export type { AsyncSqlDb, AsyncSqlRow, AsyncSqlRunner, AsyncSqlValue } from './async-db.js';
export { ensureAsyncSqlSchema } from './schema.js';
export { createAsyncAttachmentStore } from './attachment-store.js';
export { createAsyncConversationStreamStore } from './conversation-stream-store.js';
export { createAsyncEventStreamStore } from './event-stream-store.js';
export { createAsyncRunStore } from './run-store.js';
export { createAsyncSubmissionStore } from './submission-store.js';

export function createAsyncSqlStores(db: AsyncSqlDb): PersistenceStores {
	return {
		executionStore: {
			submissions: createAsyncSubmissionStore(db),
		},
		runStore: createAsyncRunStore(db),
		eventStreamStore: createAsyncEventStreamStore(db),
		conversationStreamStore: createAsyncConversationStreamStore(db),
		attachmentStore: createAsyncAttachmentStore(db),
	};
}
