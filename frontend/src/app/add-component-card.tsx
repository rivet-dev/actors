import { faPlus, Icon } from "@rivet-gg/icons";
import { type ReactNode, useState } from "react";
import { Button } from "@/components/ui/button";
import { cn } from "@/components/lib/utils";
import { useDialog } from "./use-dialog";

function AddComponentDialogTrigger({
	children,
}: {
	children: (open: () => void) => ReactNode;
}) {
	const [isOpen, setOpen] = useState(false);
	const Dialog = useDialog.AddComponent.Dialog;
	return (
		<>
			{children(() => setOpen(true))}
			<Dialog dialogProps={{ open: isOpen, onOpenChange: setOpen }} />
		</>
	);
}

export function AddComponentButton() {
	return (
		<AddComponentDialogTrigger>
			{(open) => (
				<Button
					size="sm"
					startIcon={<Icon icon={faPlus} />}
					onClick={open}
				>
					Add a component
				</Button>
			)}
		</AddComponentDialogTrigger>
	);
}

// Matches ActorBuildCard's shape so it reads as the last tile in the grid
// rather than a control that happens to sit next to it.
export function AddComponentCard() {
	return (
		<AddComponentDialogTrigger>
			{(open) => (
				<button
					type="button"
					onClick={open}
					className={cn(
						"group relative flex flex-col items-start gap-2 rounded-lg border border-foreground/10 border-dashed bg-transparent p-4 text-left transition-colors",
						"hover:border-foreground/20 hover:bg-foreground/[0.05]",
						"focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
						"min-h-[110px] cursor-pointer",
					)}
				>
					<div className="flex h-9 w-9 items-center justify-center rounded-md bg-foreground/[0.06] text-foreground/80">
						<Icon icon={faPlus} className="text-lg" />
					</div>
					<div className="font-medium text-sm leading-tight">
						Add a component
					</div>
				</button>
			)}
		</AddComponentDialogTrigger>
	);
}
