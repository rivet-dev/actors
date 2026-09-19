import { actor } from "rivetkit";
import { db } from "rivetkit/db";

export const todos = actor({
	db: db({
		onMigrate: async (db) => {
			await db.execute(
				"CREATE TABLE IF NOT EXISTS todos (id INTEGER PRIMARY KEY, title TEXT NOT NULL)",
			);
		},
	}),
	actions: {
		find: (c, query: string) =>
			c.db.executeSync<{ id: number; title: string }>(
				"SELECT id, title FROM todos WHERE title LIKE ?",
				`%${query}%`,
			),
		add: (c, title: string) =>
			c.db.transactionSync((tx) => {
				tx.executeSync("INSERT INTO todos (title) VALUES (?)", title);
				return tx.executeSync<{ id: number }>(
					"SELECT last_insert_rowid() AS id",
				)[0].id;
			}),
	},
});
