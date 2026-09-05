import type { Story } from "@ladle/react";
import "../../index.css";
import { ActorRescheduleStatus } from "./actor-status-label";

export const RescheduleCopy: Story = () => (
	<div className="mx-auto flex max-w-2xl flex-col gap-4 p-8 text-sm text-foreground">
		<div className="rounded-lg border border-border bg-card p-4">
			<p className="mb-1 font-medium">Future retry</p>
			<ActorRescheduleStatus
				rescheduleTs={new Date(Date.now() + 5 * 60 * 1000)}
			/>
		</div>
		<div className="rounded-lg border border-border bg-card p-4">
			<p className="mb-1 font-medium">Stale retry timestamp</p>
			<ActorRescheduleStatus
				rescheduleTs={new Date(Date.now() - 265 * 24 * 60 * 60 * 1000)}
			/>
		</div>
	</div>
);
