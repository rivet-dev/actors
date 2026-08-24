import { faChevronDown, faCopy, Icon } from "@rivet-gg/icons";
import { useMutation } from "@tanstack/react-query";
import { useNavigate, useSearch } from "@tanstack/react-router";
import { useState } from "react";
import { useFormContext } from "react-hook-form";
import {
	Button,
	cn,
	Dialog,
	DialogContent,
	DialogDescription,
	DialogHeader,
	DialogTitle,
	toast,
} from "@/components";
import { features } from "@/lib/features";
import { getRandomKey } from "@/lib/words";
import { queryClient } from "@/queries/global";
import { useActorsView } from "../actors-view-context-provider";
import { useEngineCompatDataProvider } from "../data-provider";
import * as ActorCreateForm from "../form/actor-create-form";

interface CreateActorSheetProps {
	open: boolean;
	onOpenChange: (open: boolean) => void;
	variant?: "actor" | "agent-os";
}

export function CreateActorSheet({
	open,
	onOpenChange,
	variant = "actor",
}: CreateActorSheetProps) {
	const dataProvider = useEngineCompatDataProvider();
	const navigate = useNavigate();
	const { mutateAsync } = useMutation({
		...dataProvider.createActorMutationOptions(),
		onSuccess: async (data) => {
			onOpenChange(false);
			const stringKeys = dataProvider
				.actorsQueryOptions({})
				.queryKey.filter((k): k is string => typeof k === "string");
			await queryClient.invalidateQueries({
				predicate: (query) =>
					stringKeys.every((k) => query.queryKey.includes(k)),
			});
			return navigate({
				to: ".",
				search: (old) => ({ ...old, modal: undefined, actorId: data }),
			});
		},
	});
	const name = useSearch({
		from: "/_context",
		select: (state) => state.n?.[0],
	});
	const { copy } = useActorsView();
	const [advancedOpen, setAdvancedOpen] = useState(false);
	const defaultRunnerNameSelector =
		ActorCreateForm.useDefaultRunnerNameSelector();

	const isAgentOs = variant === "agent-os";
	const title = isAgentOs
		? "Create agentOS instance"
		: copy.createActorModal.title(name);
	const description = isAgentOs
		? "agentOS boots an isolated VM for this key. Choose the agentOS build and a key to identify this instance."
		: copy.createActorModal.description;

	return (
		<Dialog
			open={open}
			onOpenChange={(next) => {
				onOpenChange(next);
				// Reset the disclosure when the dialog closes so it always
				// reopens in the common-case (collapsed) state.
				if (!next) setAdvancedOpen(false);
			}}
		>
			<DialogContent
				// Force a flex column layout so we can pin a sticky footer
				// while the body scrolls between header and footer. The dialog
				// is wider (max-w-2xl ≈ 672px) and taller (85vh) than the
				// default so Advanced sections have room without scroll-spam.
				className={cn(
					"max-w-2xl w-full p-0 gap-0",
					"!flex flex-col !grid-cols-none",
					"max-h-[85vh] !overflow-hidden",
				)}
			>
				<ActorCreateForm.Form
					onSubmit={async (values) => {
						await mutateAsync({
							name: values.name,
							input: values.input
								? JSON.parse(values.input)
								: undefined,
							key: values.key,
							datacenter: values.datacenter,
							runnerNameSelector:
								values.runnerNameSelector ||
								defaultRunnerNameSelector,
							crashPolicy: "destroy",
						});
					}}
					defaultValues={{
						name,
						key: getRandomKey(),
					}}
					className="flex flex-col min-h-0 flex-1"
				>
					<DialogHeader className="px-6 pt-6 pb-4 shrink-0">
						<DialogTitle>{title}</DialogTitle>
						<DialogDescription>{description}</DialogDescription>
					</DialogHeader>

					{/*
					 * Plain `overflow-y-auto` body. Simpler than ScrollArea
					 * inside a Dialog — avoids the double-scrollbar / measure
					 * issues we hit when the Dialog's own overflow fights with
					 * a nested Radix ScrollArea.
					 */}
					<div className="flex-1 min-h-0 overflow-y-auto px-6 pb-6">
						<div className="flex flex-col gap-5">
							{features.datacenter ? (
								<>
									<ActorCreateForm.PrefillActorName />
									<ActorCreateForm.PrefillRunnerName />
									<ActorCreateForm.PrefillDatacenter />
								</>
							) : null}

							{!name ? <ActorCreateForm.Build /> : null}
							<ActorCreateForm.Keys />

							<div className="mt-1 border-t border-border/60 pt-2">
								<AdvancedDisclosure
									open={advancedOpen}
									onToggle={() => setAdvancedOpen((v) => !v)}
								/>
								<CollapsibleRegion open={advancedOpen}>
									<AdvancedFields />
								</CollapsibleRegion>
							</div>
						</div>
					</div>

					<DialogFooter onCancel={() => onOpenChange(false)} />
				</ActorCreateForm.Form>
			</DialogContent>
		</Dialog>
	);
}

function AdvancedDisclosure({
	open,
	onToggle,
}: {
	open: boolean;
	onToggle: () => void;
}) {
	return (
		<button
			type="button"
			onClick={onToggle}
			aria-expanded={open}
			className={cn(
				// Quieter than the field labels above — this is a disclosure
				// affordance, not a primary field. Smaller text, muted
				// foreground, subtle hover.
				"w-full flex items-center justify-between gap-2 py-2 px-2 -mx-2 rounded-md",
				"text-xs font-normal text-muted-foreground hover:text-foreground",
				"focus-visible:outline-none focus-visible:bg-foreground/[0.06] transition-colors",
			)}
		>
			<span>Advanced options</span>
			<Icon
				icon={faChevronDown}
				className={cn(
					"size-3 transition-transform duration-300 ease-out",
					open && "rotate-180",
				)}
			/>
		</button>
	);
}

/**
 * Smooth height transition for show/hide content using the grid-rows trick.
 * Setting `grid-template-rows` between `0fr` and `1fr` lets the browser
 * animate from collapsed to natural content height without JS measurement.
 */
function CollapsibleRegion({
	open,
	children,
}: {
	open: boolean;
	children: React.ReactNode;
}) {
	return (
		<div
			aria-hidden={!open}
			className={cn(
				"grid transition-[grid-template-rows] duration-300 ease-out",
				open ? "grid-rows-[1fr]" : "grid-rows-[0fr]",
			)}
		>
			<div className="overflow-hidden">
				<div
					className={cn(
						"pt-4 transition-opacity duration-200",
						open ? "opacity-100 delay-100" : "opacity-0",
					)}
				>
					{children}
				</div>
			</div>
		</div>
	);
}

function AdvancedFields() {
	return (
		<div className="flex flex-col gap-5">
			{features.datacenter ? (
				<>
					<ActorCreateForm.Datacenter />
					<ActorCreateForm.RunnerNameSelector />
				</>
			) : (
				<ActorCreateForm.Pool />
			)}
			<ActorCreateForm.JsonInput />
		</div>
	);
}

function DialogFooter({ onCancel }: { onCancel: () => void }) {
	const { watch } = useFormContext<ActorCreateForm.FormValues>();
	const [name, key] = watch(["name", "key"]);
	const snippet =
		name && key
			? `client.${name}.getOrCreate(${JSON.stringify(key)})`
			: null;

	const copySnippet = () => {
		if (!snippet) return;
		void navigator.clipboard.writeText(snippet);
		toast.success("Copied to clipboard");
	};

	return (
		<div className="border-t border-border bg-card shrink-0">
			{snippet ? (
				<div className="px-6 pt-3 pb-3 border-b border-border/60">
					{/*
					 * Explainer line: this snippet is what the user's *app*
					 * calls, not something they have to enter here. Without
					 * this prefix, new Rivet users can read a bare code line
					 * as "the form is asking me to type this."
					 */}
					<p className="text-xs text-muted-foreground">
						Your client reaches this actor with:
					</p>
					<div className="mt-1 flex items-center gap-2 min-w-0">
						<code className="flex-1 font-mono-console text-xs text-foreground/85 truncate">
							{snippet}
						</code>
						<button
							type="button"
							onClick={copySnippet}
							className="shrink-0 rounded-md p-1.5 text-muted-foreground hover:bg-foreground/[0.06] hover:text-foreground transition-colors"
							aria-label="Copy"
						>
							<Icon icon={faCopy} className="size-3.5" />
						</button>
						<a
							href="https://www.rivet.dev/docs/clients"
							target="_blank"
							rel="noopener noreferrer"
							className="shrink-0 text-xs text-muted-foreground hover:text-foreground transition-colors"
						>
							Docs ↗
						</a>
					</div>
				</div>
			) : null}
			<div className="flex items-center justify-end gap-2 px-6 py-3">
				<Button
					type="button"
					variant="ghost"
					size="sm"
					onClick={onCancel}
				>
					Cancel
				</Button>
				<ActorCreateForm.Submit allowPristine type="submit" size="sm">
					Create
				</ActorCreateForm.Submit>
			</div>
		</div>
	);
}
