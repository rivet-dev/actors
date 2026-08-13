import { faCheck, Icon } from "@rivet-gg/icons";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import {
	Command,
	CommandEmpty,
	CommandGroup,
	CommandInput,
	CommandItem,
	CommandList,
} from "@/components/ui/command";
import {
	Popover,
	PopoverContent,
	PopoverTrigger,
} from "@/components/ui/popover";
import { cn } from "@/components/lib/utils";

/**
 * Minimal shape of a managed pool needed to render the switcher. Kept
 * structural (rather than importing the SDK type) so the component can be
 * driven from fixtures in stories without the cloud data-provider stack.
 */
export interface PoolSwitcherPool {
	name: string;
	config?: { displayName?: string };
}

/** Human label for a pool, falling back to its machine name. */
export function poolDisplayName(pool: PoolSwitcherPool): string {
	return pool.config?.displayName || pool.name;
}

/**
 * Resolves which pool a surface should show given the `?pool=` search param and
 * the available pools. Prefers the requested pool, then a pool literally named
 * "default", then the first pool, then the "default" string as a last resort.
 */
export function resolvePoolName(
	pools: PoolSwitcherPool[],
	requested: unknown,
): string {
	if (
		typeof requested === "string" &&
		pools.some((pool) => pool.name === requested)
	) {
		return requested;
	}
	if (pools.some((pool) => pool.name === "default")) {
		return "default";
	}
	return pools[0]?.name ?? "default";
}

/**
 * Static header text shown before the pool switcher button. The pool name lives
 * in the switcher button itself, so the header reads e.g. "Pool Logs [Backend v]".
 * With a single pool the switcher is hidden and the generic title ("Logs" /
 * "Compute") is used instead.
 */
export function poolHeaderText(
	pools: PoolSwitcherPool[],
	suffix: string,
	generic: string,
): string {
	return pools.length <= 1 ? generic : `Pool ${suffix}`;
}

// Up/down "unfold" chevron, matching the segment switchers in context-switcher.
function UnfoldIcon({ className }: { className?: string }) {
	return (
		<svg
			viewBox="0 0 24 24"
			width="14"
			height="14"
			stroke="currentColor"
			strokeWidth="2"
			strokeLinecap="round"
			strokeLinejoin="round"
			fill="none"
			className={cn("size-3 opacity-60", className)}
			aria-hidden
		>
			<path d="m7 15 5 5 5-5" />
			<path d="m7 9 5-5 5 5" />
		</svg>
	);
}

interface PoolSwitcherProps {
	pools: PoolSwitcherPool[];
	value: string;
	onChange: (poolName: string) => void;
	className?: string;
}

/**
 * Compact switch button placed to the right of a pool-scoped title. Opens a
 * searchable list of the namespace's managed pools. Callers should only render
 * this when there is more than one pool.
 */
export function PoolSwitcher({
	pools,
	value,
	onChange,
	className,
}: PoolSwitcherProps) {
	const [open, setOpen] = useState(false);
	// Controls which item cmdk highlights. Reset to the current pool each time
	// the popover opens so it opens focused on the active selection.
	const [commandValue, setCommandValue] = useState(value);
	const selected = pools.find((pool) => pool.name === value);
	const label = selected ? poolDisplayName(selected) : value;

	return (
		<Popover
			open={open}
			onOpenChange={(next) => {
				setOpen(next);
				if (next) setCommandValue(value);
			}}
		>
			<PopoverTrigger asChild>
				<Button
					variant="outline"
					size="sm"
					aria-label={`Switch pool (current: ${label})`}
					className={cn(
						"h-auto gap-1.5 rounded-lg py-1 pl-3 pr-2 font-medium text-foreground data-[state=open]:bg-foreground/[0.06]",
						className,
					)}
				>
					<span className="truncate max-w-[16rem]">{label}</span>
					<UnfoldIcon />
				</Button>
			</PopoverTrigger>
			<PopoverContent
				className="p-0 w-56"
				align="start"
				closeAnimation={false}
			>
				<Command
					loop
					value={commandValue}
					onValueChange={setCommandValue}
				>
					<CommandInput placeholder="Find pool..." />
					<CommandList>
						<CommandEmpty>No pools found.</CommandEmpty>
						<CommandGroup heading="Pools">
							{pools.map((pool) => {
								const isCurrent = pool.name === value;
								return (
									<CommandItem
										key={pool.name}
										value={pool.name}
										keywords={[poolDisplayName(pool)]}
										onSelect={() => {
											setOpen(false);
											if (!isCurrent) {
												onChange(pool.name);
											}
										}}
									>
										<Icon
											icon={faCheck}
											className={cn(
												"mr-2 size-3 shrink-0 text-primary",
												isCurrent
													? "opacity-100"
													: "opacity-0",
											)}
										/>
										<span className="truncate flex-1">
											{poolDisplayName(pool)}
										</span>
									</CommandItem>
								);
							})}
						</CommandGroup>
					</CommandList>
				</Command>
			</PopoverContent>
		</Popover>
	);
}
