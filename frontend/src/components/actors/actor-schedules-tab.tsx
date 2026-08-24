import { faCalendar, faSpinnerThird, faTrash, Icon } from "@rivet-gg/icons";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Fragment, type ReactNode, useEffect, useState } from "react";
import { toast } from "sonner";
import { WithTooltip } from "@/components";
import { Button } from "@/components/ui/button";
import {
	Table,
	TableBody,
	TableCell,
	TableHead,
	TableHeader,
	TableRow,
} from "@/components/ui/table";
import { formatValue } from "@/lib/format-value";
import { cn } from "../lib/utils";
import {
	type InspectorSchedule,
	type InspectorScheduleFire,
	useActorInspector,
} from "./actor-inspector-context";
import {
	describeCronExpression,
	formatDuration,
	formatSchedule,
} from "./actor-schedules-format";
import type { ActorId } from "./queries";

export function ActorSchedulesTab({ actorId }: { actorId: ActorId }) {
	const inspector = useActorInspector();
	const [selectedId, setSelectedId] = useState<string>();
	const [now, setNow] = useState(() => Date.now());

	const { data: schedules = [], isLoading } = useQuery(
		inspector.actorSchedulesQueryOptions(actorId),
	);

	useEffect(() => {
		const timer = window.setInterval(() => setNow(Date.now()), 1_000);
		return () => window.clearInterval(timer);
	}, []);

	if (!inspector.features.schedules.supported) {
		return (
			<div className="flex flex-1 items-center justify-center p-8 text-sm text-muted-foreground">
				{inspector.features.schedules.message}
			</div>
		);
	}

	return (
		<div
			className="flex min-h-0 flex-1 flex-col"
			data-testid="schedules-tab"
		>
			{isLoading ? (
				<div className="flex flex-1 items-center justify-center text-muted-foreground">
					<Icon icon={faSpinnerThird} className="mr-2 animate-spin" />
					Loading schedules…
				</div>
			) : schedules.length === 0 ? (
				<EmptySchedules />
			) : (
				<Table containerClassName="flex-1 min-h-0 bg-card">
					<TableHeader className="sticky top-0 z-10 bg-card">
						<TableRow>
							<TableHead>Name / ID</TableHead>
							<TableHead>Action</TableHead>
							<TableHead>Schedule</TableHead>
							<TableHead>Next run</TableHead>
							<TableHead>Last run</TableHead>
						</TableRow>
					</TableHeader>
					<TableBody>
						{schedules.map((schedule) => {
							const isSelected = schedule.id === selectedId;
							const toggle = () =>
								setSelectedId(
									isSelected ? undefined : schedule.id,
								);

							return (
								<Fragment
									key={`${schedule.kind}:${schedule.id}`}
								>
									<TableRow
										isClickable
										data-state={
											isSelected ? "selected" : undefined
										}
										aria-expanded={isSelected}
										tabIndex={0}
										onClick={toggle}
										onKeyDown={(event) => {
											if (
												event.key === "Enter" ||
												event.key === " "
											) {
												event.preventDefault();
												toggle();
											}
										}}
										data-testid={`schedule-row-${schedule.id}`}
									>
										<TableCell className="max-w-56">
											<ScheduleName schedule={schedule} />
										</TableCell>
										<TableCell className="font-mono text-xs">
											{schedule.action}
										</TableCell>
										<TableCell>
											<ScheduleValue
												schedule={schedule}
											/>
										</TableCell>
										<TableCell>
											<RelativeTime
												timestamp={schedule.nextRunAt}
												now={now}
											/>
										</TableCell>
										<TableCell className="text-muted-foreground">
											{schedule.lastRunAt ? (
												<RelativeTime
													timestamp={
														schedule.lastRunAt
													}
													now={now}
												/>
											) : (
												"—"
											)}
										</TableCell>
									</TableRow>
									{isSelected && (
										<TableRow className="bg-muted/20 hover:bg-muted/20">
											<TableCell
												colSpan={5}
												className="p-0"
											>
												<ScheduleDetails
													actorId={actorId}
													schedule={schedule}
													now={now}
													onClose={() =>
														setSelectedId(undefined)
													}
												/>
											</TableCell>
										</TableRow>
									)}
								</Fragment>
							);
						})}
					</TableBody>
				</Table>
			)}
		</div>
	);
}

function EmptySchedules() {
	return (
		<div className="flex flex-1 flex-col items-center justify-center p-8 text-center">
			<div className="mb-3 flex size-11 items-center justify-center rounded-full bg-muted">
				<Icon icon={faCalendar} className="text-muted-foreground" />
			</div>
			<h3 className="font-medium">No schedules yet</h3>
			<p className="mt-1 max-w-sm text-sm text-muted-foreground">
				Pending one-time and recurring schedules will appear here.
			</p>
		</div>
	);
}

function ScheduleName({ schedule }: { schedule: InspectorSchedule }) {
	if (schedule.name) {
		return <div className="truncate font-medium">{schedule.name}</div>;
	}

	const shortId =
		schedule.id.length > 16
			? `${schedule.id.slice(0, 8)}…${schedule.id.slice(-4)}`
			: schedule.id;
	return (
		<WithTooltip
			content={schedule.id}
			trigger={
				<code className="cursor-help text-xs" tabIndex={0}>
					{shortId}
				</code>
			}
		/>
	);
}

function ScheduleValue({ schedule }: { schedule: InspectorSchedule }) {
	if (schedule.kind !== "cron") {
		return <span>{formatSchedule(schedule)}</span>;
	}

	const expression = schedule.expression ?? "";
	return (
		<WithTooltip
			content={
				<div className="space-y-1">
					<div>{describeCronExpression(expression)}</div>
					<div className="text-xs text-muted-foreground">
						Timezone: {schedule.timezone ?? "UTC"}
					</div>
				</div>
			}
			trigger={
				<code
					className="cursor-help border-b border-dotted border-muted-foreground/60 text-xs"
					tabIndex={0}
				>
					{expression}
				</code>
			}
		/>
	);
}

function ScheduleDetails({
	actorId,
	schedule,
	now,
	onClose,
}: {
	actorId: ActorId;
	schedule: InspectorSchedule;
	now: number;
	onClose: () => void;
}) {
	const inspector = useActorInspector();
	const [confirming, setConfirming] = useState(false);
	const isRecurring = schedule.kind !== "at";
	const { data: history = [], isLoading } = useQuery({
		...inspector.actorScheduleHistoryQueryOptions(actorId, schedule.id),
		enabled: isRecurring,
	});
	const deletion = useMutation({
		...inspector.actorScheduleDeleteMutation(actorId),
		onSuccess: (deleted) => {
			if (deleted) toast.success("Schedule deleted");
			onClose();
		},
		onError: (error) => toast.error(error.message),
	});

	return (
		<div className="grid gap-5 border-t p-4 md:grid-cols-2">
			<div className={cn("space-y-5", !isRecurring && "md:col-span-2")}>
				<dl className="grid grid-cols-[6rem_1fr] gap-x-4 gap-y-2 text-sm">
					<Detail label="ID">
						<code className="break-all text-xs">{schedule.id}</code>
					</Detail>
					<Detail label="Next run">
						<div>{formatTimestamp(schedule.nextRunAt)}</div>
						<div className="text-xs text-muted-foreground">
							<RelativeTime
								timestamp={schedule.nextRunAt}
								now={now}
							/>
						</div>
					</Detail>
					{schedule.lastRunAt && (
						<Detail label="Last run">
							{formatTimestamp(schedule.lastRunAt)}
						</Detail>
					)}
					{schedule.maxHistory != null && (
						<Detail label="History">
							{schedule.maxHistory === 0
								? "Disabled"
								: `Keep ${schedule.maxHistory} entries`}
						</Detail>
					)}
				</dl>

				<section>
					<h3 className="mb-2 text-xs font-medium uppercase tracking-wider text-muted-foreground">
						Arguments
					</h3>
					<pre className="max-h-40 overflow-auto rounded-md border bg-card p-3 text-xs">
						{formatValue(schedule.args, true)}
					</pre>
				</section>
			</div>

			{isRecurring && (
				<section>
					<h3 className="mb-2 text-xs font-medium uppercase tracking-wider text-muted-foreground">
						Recent runs
					</h3>
					{isLoading ? (
						<div className="py-6 text-center text-sm text-muted-foreground">
							<Icon
								icon={faSpinnerThird}
								className="mr-2 animate-spin"
							/>
							Loading history…
						</div>
					) : history.length ? (
						<HistoryList history={history} now={now} />
					) : (
						<div className="rounded-md border border-dashed p-5 text-center text-sm text-muted-foreground">
							No runs recorded yet.
						</div>
					)}
				</section>
			)}

			<div className="flex justify-end border-t pt-3 md:col-span-2">
				<Button
					variant={confirming ? "destructive" : "outline"}
					size="sm"
					disabled={deletion.isPending}
					onClick={() => {
						if (!confirming) {
							setConfirming(true);
							return;
						}
						deletion.mutate({
							scheduleId: schedule.id,
							kind: schedule.kind,
						});
					}}
					startIcon={
						<Icon
							icon={deletion.isPending ? faSpinnerThird : faTrash}
							className={cn(deletion.isPending && "animate-spin")}
						/>
					}
				>
					{confirming
						? schedule.kind === "at"
							? "Confirm cancellation"
							: "Confirm deletion"
						: schedule.kind === "at"
							? "Cancel schedule"
							: "Delete schedule"}
				</Button>
			</div>
		</div>
	);
}

function Detail({ label, children }: { label: string; children: ReactNode }) {
	return (
		<>
			<dt className="text-muted-foreground">{label}</dt>
			<dd className="min-w-0 break-words">{children}</dd>
		</>
	);
}

function HistoryList({
	history,
	now,
}: {
	history: InspectorScheduleFire[];
	now: number;
}) {
	return (
		<div className="max-h-56 divide-y overflow-y-auto rounded-md border bg-card">
			{history.map((fire, index) => (
				<div
					key={`${fire.firedAt}:${index}`}
					className="grid grid-cols-[6rem_1fr_auto] items-start gap-3 p-3 text-xs"
				>
					<ResultBadge result={fire.result} />
					<div>
						<div>{formatTimestamp(fire.firedAt)}</div>
						{fire.error && (
							<div className="mt-1 text-muted-foreground">
								{fire.error.code}: {fire.error.message}
							</div>
						)}
					</div>
					<div className="text-right text-muted-foreground">
						{formatDuration(
							(fire.finishedAt ?? now) - fire.firedAt,
						)}
					</div>
				</div>
			))}
		</div>
	);
}

function ResultBadge({ result }: { result: InspectorScheduleFire["result"] }) {
	return (
		<div className="flex items-center gap-1.5 capitalize">
			<span
				className={cn(
					"size-1.5 rounded-full",
					result === "ok" && "bg-emerald-500",
					result === "error" && "bg-destructive",
					result === "running" && "animate-pulse bg-blue-500",
					result === "skipped" && "bg-amber-500",
				)}
			/>
			{result === "ok" ? "Success" : result}
		</div>
	);
}

function RelativeTime({ timestamp, now }: { timestamp: number; now: number }) {
	const delta = timestamp - now;
	const future = delta >= 0;
	const absolute = Math.abs(delta);
	let value: string;
	if (absolute < 60_000)
		value = `${Math.max(1, Math.round(absolute / 1_000))}s`;
	else if (absolute < 3_600_000) value = `${Math.round(absolute / 60_000)}m`;
	else if (absolute < 86_400_000)
		value = `${Math.round(absolute / 3_600_000)}h`;
	else value = `${Math.round(absolute / 86_400_000)}d`;
	return (
		<time
			dateTime={new Date(timestamp).toISOString()}
			title={formatTimestamp(timestamp)}
		>
			{future ? `in ${value}` : `${value} ago`}
		</time>
	);
}

function formatTimestamp(timestamp: number): string {
	return new Intl.DateTimeFormat(undefined, {
		dateStyle: "medium",
		timeStyle: "medium",
	}).format(timestamp);
}
