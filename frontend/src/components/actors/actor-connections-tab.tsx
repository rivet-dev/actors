import { faPlug, faSpinnerThird, Icon } from "@rivet-gg/icons";
import { useQuery } from "@tanstack/react-query";
import { DiscreteCopyButton, LiveBadge, ScrollArea } from "@/components";
import {
	Table,
	TableBody,
	TableCell,
	TableHead,
	TableHeader,
	TableRow,
} from "@/components/ui/table";
import { useActorInspector } from "./actor-inspector-context";
import { Info } from "./actor-state-tab";
import { ActorObjectInspector } from "./console/actor-inspector";
import { useDataProvider } from "./data-provider";
import type { ActorId } from "./queries";

interface ActorConnectionsTabProps {
	actorId: ActorId;
}

/**
 * Shape of the CBOR-decoded `details` payload sent by RivetKit. Kept loose on
 * purpose: older runtimes send `type` instead of `connectionType`, and the
 * inspector should still render whatever it gets.
 */
interface ConnectionDetails {
	connectionType?: string | null;
	type?: string | null;
	params?: unknown;
	state?: unknown;
	stateEnabled?: boolean;
	subscriptions?: number;
	isHibernatable?: boolean;
}

export function ActorConnectionsTab({ actorId }: ActorConnectionsTabProps) {
	const { data: destroyedAt } = useQuery(
		useDataProvider().actorDestroyedAtQueryOptions(actorId),
	);

	const inspector = useActorInspector();
	const { data = [], isLoading } = useQuery(
		inspector.actorConnectionsQueryOptions(actorId),
	);

	if (destroyedAt) {
		return (
			<div className="flex-1 flex items-center justify-center h-full text-center">
				Connections Preview is unavailable for inactive Actors.
			</div>
		);
	}

	if (isLoading) {
		return (
			<Info>
				<div className="flex items-center">
					<Icon icon={faSpinnerThird} className="animate-spin mr-2" />
					Loading Connections...
				</div>
			</Info>
		);
	}

	return (
		<div className="flex h-full min-h-0 flex-1 flex-col">
			<div className="flex justify-between items-center gap-1 border-b px-2 h-9">
				<LiveBadge />
				<div className="text-xs text-muted-foreground">
					{data.length}{" "}
					{data.length === 1 ? "connection" : "connections"}
				</div>
			</div>
			{data.length === 0 ? (
				<EmptyConnections />
			) : (
				<ScrollArea className="flex-1 w-full min-h-0">
					<Table>
						<TableHeader>
							<TableRow>
								<TableHead>Connection ID</TableHead>
								<TableHead>Type</TableHead>
								<TableHead>Hibernatable</TableHead>
								<TableHead>Params</TableHead>
								<TableHead>State</TableHead>
							</TableRow>
						</TableHeader>
						<TableBody>
							{data.map((connection) => {
								const details = (connection.details ??
									{}) as ConnectionDetails;
								const type =
									details.connectionType ??
									details.type ??
									"—";
								return (
									<TableRow
										key={connection.id}
										className="align-top"
									>
										<TableCell className="whitespace-nowrap align-top">
											<DiscreteCopyButton
												size="xs"
												className="font-mono-console text-xs -ml-2"
												value={connection.id}
											>
												{connection.id}
											</DiscreteCopyButton>
										</TableCell>
										<TableCell className="text-muted-foreground align-top">
											{type}
										</TableCell>
										<TableCell className="text-muted-foreground align-top">
											{details.isHibernatable
												? "Yes"
												: "No"}
										</TableCell>
										<TableCell className="w-1/4 min-w-48 align-top">
											<ConnectionValue
												name="params"
												value={details.params}
											/>
										</TableCell>
										<TableCell className="w-1/4 min-w-48 align-top">
											{details.stateEnabled === false ? (
												<span className="text-muted-foreground">
													Disabled
												</span>
											) : (
												<ConnectionValue
													name="state"
													value={details.state}
												/>
											)}
										</TableCell>
									</TableRow>
								);
							})}
						</TableBody>
					</Table>
				</ScrollArea>
			)}
		</div>
	);
}

function ConnectionValue({ name, value }: { name: string; value: unknown }) {
	if (value === undefined || value === null) {
		return <span className="text-muted-foreground">—</span>;
	}
	return (
		<ActorObjectInspector
			name={name}
			data={value}
			expandPaths={["$"]}
			className="text-xs"
		/>
	);
}

function EmptyConnections() {
	return (
		<div className="flex flex-1 flex-col items-center justify-center p-8 text-center">
			<div className="mb-3 flex size-11 items-center justify-center rounded-full bg-muted">
				<Icon icon={faPlug} className="text-muted-foreground" />
			</div>
			<h3 className="font-medium">No active connections</h3>
			<p className="mt-1 max-w-sm text-sm text-muted-foreground">
				Clients connected to this actor, their params, and connection
				state will appear here.
			</p>
		</div>
	);
}
