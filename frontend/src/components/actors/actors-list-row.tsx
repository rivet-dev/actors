import { faCopy, Icon } from "@rivet-gg/icons";
import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { memo } from "react";
import {
	Button,
	CopyTrigger,
	cn,
	RelativeTime,
	Skeleton,
	SmallText,
	WithTooltip,
} from "@/components";
import { useFiltersValue } from "./actor-filters-context";
import { ActorRegion } from "./actor-region";
import {
	ActorStatusIndicator,
	QueriedActorStatusIndicator,
} from "./actor-status-indicator";
import { QueriedActorStatusLabel } from "./actor-status-label";
import { useDataProvider } from "./data-provider";
import type { ActorId } from "./queries";

export function actorsTableGridTemplate(
	showIds: boolean,
	showDatacenter: boolean,
) {
	const parts: string[] = ["44px"];
	if (showIds) parts.push("96px");
	if (showDatacenter) parts.push("64px");
	parts.push("minmax(0,1fr)");
	parts.push("72px");
	return parts.join(" ");
}

export function useActorsTableColumns() {
	const filters = useFiltersValue();
	const showIds = filters.showIds?.value.includes("1") ?? false;
	const showDatacenter = filters.showDatacenter?.value.includes("1") ?? false;
	return { showIds, showDatacenter };
}

interface ActorsListRowProps {
	className?: string;
	actorId: ActorId;
	isCurrent?: boolean;
}

export const ActorsListRow = memo(
	({ className, actorId, isCurrent }: ActorsListRowProps) => {
		const { showIds, showDatacenter } = useActorsTableColumns();
		const template = actorsTableGridTemplate(showIds, showDatacenter);

		return (
			<Button
				className={cn(
					"relative h-8 w-full grid items-center group border-l-0 border-r-0 border-t-0 border-b hover:border-foreground/10 rounded-none px-3 text-xs gap-3",
					isCurrent &&
						"bg-foreground/[0.08] bg-clip-padding hover:bg-foreground/[0.10] text-foreground",
					className,
				)}
				variant="outline"
				asChild
			>
				<Link
					to="."
					search={(search: Record<string, unknown>) => ({
						...search,
						actorId,
					})}
					style={{ gridTemplateColumns: template }}
				>
					<WithTooltip
						delayDuration={0}
						trigger={
							<div className="flex justify-start items-center">
								<QueriedActorStatusIndicator
									actorId={actorId}
								/>
							</div>
						}
						content={
							<div className="flex flex-col">
								<QueriedActorStatusLabel
									actorId={actorId}
									showAdditionalInfo
								/>
							</div>
						}
					/>
					{showIds ? <Id actorId={actorId} /> : null}
					{showDatacenter ? <Datacenter actorId={actorId} /> : null}
					<Tags actorId={actorId} />
					<Timestamp actorId={actorId} />
				</Link>
			</Button>
		);
	},
);

function Id({ actorId }: { actorId: ActorId }) {
	const shortId = actorId.includes("-")
		? actorId.split("-")[0]
		: actorId.substring(0, 8);
	return (
		<CopyTrigger value={actorId}>
			<span className="group/id inline-flex items-center gap-1 min-w-0 max-w-full cursor-pointer text-muted-foreground hover:text-foreground tabular-nums font-mono-console transition-colors">
				<span className="truncate min-w-0">{shortId}</span>
				<Icon
					icon={faCopy}
					className="opacity-0 group-hover/id:opacity-100 transition-opacity shrink-0 size-2.5"
				/>
			</span>
		</CopyTrigger>
	);
}

function Datacenter({ actorId }: { actorId: ActorId }) {
	const { data: datacenter, isLoading } = useQuery({
		...useDataProvider().actorDatacenterQueryOptions(actorId),
	});

	return (
		<SmallText className="text-foreground min-w-0 truncate">
			{isLoading ? (
				<Skeleton className="h-5 w-10" />
			) : datacenter ? (
				<ActorRegion regionId={datacenter} />
			) : (
				"-"
			)}
		</SmallText>
	);
}

function Tags({ actorId }: { actorId: ActorId }) {
	const { data, isLoading } = useQuery(
		useDataProvider().actorKeysQueryOptions(actorId),
	);

	if (isLoading) {
		return (
			<SmallText className="text-foreground truncate min-w-0 max-w-full inline-flex items-center">
				<Skeleton className="h-5 w-10" />
			</SmallText>
		);
	}

	if (!data) {
		return (
			<SmallText className="text-foreground truncate min-w-0 max-w-full inline-flex items-center">
				-
			</SmallText>
		);
	}

	return (
		<div>
			<CopyTrigger value={data}>
				<span className="group/key inline-flex items-center gap-1 min-w-0 max-w-full cursor-pointer text-foreground transition-colors">
					<span className="truncate min-w-0">{data}</span>
					<Icon
						icon={faCopy}
						className="opacity-0 group-hover/key:opacity-100 transition-opacity shrink-0 size-3"
					/>
				</span>
			</CopyTrigger>
		</div>
	);
}

function Timestamp({ actorId }: { actorId: ActorId }) {
	const { data: { createTs, destroyTs } = {}, isLoading } = useQuery(
		useDataProvider().actorQueryOptions(actorId),
	);

	const ts = destroyTs || createTs;

	const timestamp = ts ? new Date(ts) : null;

	return (
		<span className="text-right text-xs font-normal text-muted-foreground justify-end inline-flex tabular-nums">
			{isLoading ? (
				<Skeleton className="h-4 w-10" />
			) : timestamp ? (
				<WithTooltip
					trigger={<RelativeTime time={timestamp} />}
					content={`Created at ${timestamp.toLocaleString()}`}
				/>
			) : (
				<span>-</span>
			)}
		</span>
	);
}

export function ActorsListHeader() {
	const { showIds, showDatacenter } = useActorsTableColumns();
	const template = actorsTableGridTemplate(showIds, showDatacenter);

	return (
		<div className="sticky top-9 z-[1] bg-card border-b border-foreground/15">
			<div
				className="bg-foreground/[0.04] grid items-center gap-3 px-3 h-7 text-[10px] font-medium uppercase tracking-wide text-muted-foreground"
				style={{ gridTemplateColumns: template }}
			>
				<div>Status</div>
				{showIds ? <div>ID</div> : null}
				{showDatacenter ? <div>Region</div> : null}
				<div>Key</div>
				<div className="text-right">Created</div>
			</div>
		</div>
	);
}

function SkeletonContent() {
	const { showIds, showDatacenter } = useActorsTableColumns();
	const template = actorsTableGridTemplate(showIds, showDatacenter);

	return (
		<div
			className="grid items-center w-full gap-3"
			style={{ gridTemplateColumns: template }}
		>
			<div className="flex justify-center">
				<ActorStatusIndicator status="unknown" />
			</div>
			{showIds ? <Skeleton className="h-4 w-16" /> : null}
			{showDatacenter ? <Skeleton className="h-4 w-10" /> : null}
			<Skeleton className="h-4 w-32" />
			<Skeleton className="h-4 w-10 justify-self-end" />
		</div>
	);
}

export function ActorsListRowSkeleton() {
	return (
		<div className="border-b flex items-center px-3 h-8 text-xs relative">
			<SkeletonContent />
		</div>
	);
}
