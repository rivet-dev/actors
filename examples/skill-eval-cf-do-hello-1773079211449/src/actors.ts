import { actor, setup } from "rivetkit";

export const myActor = actor({
	state: {},
	actions: {
		sayHello: (c) => {
			const result = c.db
				.exec("SELECT 'Hello, World!' as greeting")
				.toArray() as { greeting: string }[];
			return result[0].greeting;
		},
	},
});

export const registry = setup({
	use: { myActor },
});
