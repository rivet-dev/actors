import type { Story } from "@ladle/react";
import { useState } from "react";
import "../../.ladle/ladle.css";
import {
	PoolSwitcher,
	type PoolSwitcherPool,
	poolHeaderText,
} from "./pool-switcher";

function Frame({ children }: { children: React.ReactNode }) {
	return (
		<div className="bg-background min-h-screen p-12">
			<div className="max-w-3xl space-y-8">{children}</div>
		</div>
	);
}

// Mirrors how a page composes the pool-scoped title with the switcher: the
// switcher only renders when there is more than one pool, otherwise the title
// stays generic. This is the real integration unit, not the switcher alone.
function TitleWithSwitcher({
	pools,
	suffix,
	generic,
}: {
	pools: PoolSwitcherPool[];
	suffix: string;
	generic: string;
}) {
	const [value, setValue] = useState(pools[0]?.name ?? "default");
	return (
		<div className="flex items-center gap-1.5">
			<h1 className="text-2xl font-semibold text-foreground">
				{poolHeaderText(pools, suffix, generic)}
			</h1>
			{pools.length > 1 ? (
				<PoolSwitcher
					pools={pools}
					value={value}
					onChange={setValue}
				/>
			) : null}
		</div>
	);
}

const SINGLE: PoolSwitcherPool[] = [
	{ name: "default", config: { displayName: "Default" } },
];

const TWO: PoolSwitcherPool[] = [
	{ name: "default", config: { displayName: "Default" } },
	{ name: "backend", config: { displayName: "Backend" } },
];

const MANY: PoolSwitcherPool[] = [
	{ name: "default", config: { displayName: "Default" } },
	{ name: "backend", config: { displayName: "Backend Workers" } },
	{ name: "gpu", config: { displayName: "GPU Inference" } },
	{ name: "batch", config: { displayName: "Batch Jobs" } },
	// No config: falls back to the machine name.
	{ name: "legacy-pool" },
];

const LONG: PoolSwitcherPool[] = [
	{ name: "default", config: { displayName: "Default" } },
	{
		name: "long",
		config: {
			displayName:
				"Extremely long managed pool display name that should truncate gracefully in both the title and the switcher list",
		},
	},
];

// Single pool: the switcher is not rendered and the title stays generic.
export const SinglePool: Story = () => (
	<Frame>
		<TitleWithSwitcher pools={SINGLE} suffix="Logs" generic="Logs" />
	</Frame>
);

export const TwoPoolsLogs: Story = () => (
	<Frame>
		<TitleWithSwitcher pools={TWO} suffix="Logs" generic="Logs" />
	</Frame>
);

export const TwoPoolsConfig: Story = () => (
	<Frame>
		<TitleWithSwitcher pools={TWO} suffix="Config" generic="Compute" />
	</Frame>
);

export const ManyPools: Story = () => (
	<Frame>
		<TitleWithSwitcher pools={MANY} suffix="Logs" generic="Logs" />
	</Frame>
);

export const LongDisplayName: Story = () => (
	<Frame>
		<TitleWithSwitcher pools={LONG} suffix="Logs" generic="Logs" />
	</Frame>
);
