import { setup } from "rivetkit";
import { notificationsActor } from "./sse";

export const registry = setup({
	use: { notifications: notificationsActor },
});

registry.start();
