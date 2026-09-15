import { faBooks, Icon } from "@rivet-gg/icons";
import { Button, ScrollArea } from "@/components";
import { ActorMetrics } from "./actor-metrics";
import type { ActorId } from "./queries";

interface ActorMetricsTabProps {
	actorId: ActorId;
}

export function ActorMetricsTab(props: ActorMetricsTabProps) {
	return (
		<ScrollArea className="overflow-auto h-full">
			<div className="flex justify-end items-center gap-1 border-b sticky top-0 px-2 z-[1] h-9">
				<Button
					variant="outline"
					size="sm"
					startIcon={<Icon icon={faBooks} />}
				>
					Documentation
				</Button>
			</div>
			<ActorMetrics {...props} />
		</ScrollArea>
	);
}
