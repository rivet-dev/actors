import {
	faArrowUpRightFromSquare,
	faBookOpen,
	faMagnifyingGlass,
	faQuestionSquare,
	faSidebarFlip,
	Icon,
} from "@rivet-gg/icons";
import { useHotkey } from "@tanstack/react-hotkeys";
import {
	useInfiniteQuery,
	useQuery,
	useQueryClient,
	useSuspenseInfiniteQuery,
} from "@tanstack/react-query";
import { useNavigate, useSearch } from "@tanstack/react-router";
import { useVirtualizer } from "@tanstack/react-virtual";
import {
	type RefObject,
	Suspense,
	useCallback,
	useEffect,
	useRef,
	useState,
} from "react";
import { toast } from "sonner";
import { useLocalStorage } from "usehooks-ts";
import { RECORDS_PER_PAGE } from "@/app/data-providers/default-data-provider";
import {
	Button,
	CursorTooltipGroup,
	CursorTooltipShortcut,
	CursorTooltipTrigger,
	Dialog,
	DialogContent,
	DialogDescription,
	DialogFooter,
	DialogHeader,
	DialogTitle,
	FiltersDisplay,
	Input,
	ls,
	type OnFiltersChange,
	ScrollArea,
	ShimmerLine,
	SmallText,
} from "@/components";
import { CodePreview } from "../code-preview/code-preview";
import { VisibilitySensor } from "../visibility-sensor";
import { useActorsFilters, useFiltersValue } from "./actor-filters-context";
import { useActorsLayout } from "./actors-layout-context";
import {
	ActorsListHeader,
	ActorsListRow,
	ActorsListRowSkeleton,
} from "./actors-list-row";
import { useActorsView } from "./actors-view-context-provider";
import { CreateActorButton } from "./create-actor-button";
import { useDataProvider } from "./data-provider";
import { NoProvidersAlert } from "./no-providers-alert";

export function ActorsList() {
	const viewportRef = useRef<HTMLDivElement>(null);

	return (
		<ScrollArea
			className="h-full w-full @container/main"
			viewportRef={viewportRef}
		>
			<TopBar />
			<Page1Poller />
			<Suspense fallback={<ListSkeleton />}>
				<ActorsTableHeaderGate />
				<List viewportRef={viewportRef} />
				<Pagination />
			</Suspense>
		</ScrollArea>
	);
}

function ActorNameLabel() {
	const { n } = useSearch({ from: "/_context" });
	const buildId = n?.[0];
	const { data: builds = [] } = useInfiniteQuery(
		useDataProvider().buildsQueryOptions(),
	);

	if (!buildId) return null;

	const build = builds.find((b) => b.id === buildId);
	const meta = build?.name?.metadata as Record<string, unknown> | undefined;
	const displayName =
		typeof meta?.name === "string" ? meta.name : (buildId ?? "");

	return (
		<span className="text-sm font-medium text-foreground truncate min-w-0">
			{displayName}
		</span>
	);
}

function Page1Poller() {
	const { n } = useSearch({ from: "/_context" });
	const filters = useFiltersValue({ onlyStatic: true });
	useQuery(useDataProvider().actorsListPage1PollQueryOptions({ n, filters }));
	return null;
}

function TopBar() {
	const { isDetailsColCollapsed, detailsRef } = useActorsLayout();
	const { n } = useSearch({ from: "/_context" });
	const filters = useFiltersValue({ onlyStatic: true });
	const { data: actors = [], isLoading } = useInfiniteQuery(
		useDataProvider().actorsListQueryOptions({ n, filters }),
	);
	const filtersCount = Object.values(filters).length;

	// When there are no instances and the user hasn't applied any filter,
	// suppress both the "+" button and the search/display row so the empty
	// state below is the only call-to-action. Filters keep the row visible
	// because the user still needs a way to clear them.
	const showInstanceTools =
		isLoading || actors.length > 0 || filtersCount > 0;

	return (
		<div className="col-span-full border-b sticky top-0 bg-card z-[1]">
			<div className="flex items-center px-3 gap-2 h-9">
				<ActorNameLabel />
				<CursorTooltipGroup className="ml-auto flex items-center gap-1 shrink-0">
					{showInstanceTools ? (
						<>
							<CursorTooltipTrigger content="Display options">
								<Display />
							</CursorTooltipTrigger>
							<InstanceSearchTrigger />
							<CreateActorButton
								iconOnly
								label="Create Instance"
								renderTooltip={(trigger, content) => (
									<CursorTooltipTrigger content={content}>
										{trigger}
									</CursorTooltipTrigger>
								)}
							/>
						</>
					) : null}
					{isDetailsColCollapsed ? (
						<CursorTooltipTrigger content="Expand details column">
							<Button
								onClick={() => detailsRef.current?.expand()}
								variant="ghost"
								size="icon-sm"
								aria-label="Expand details column"
							>
								<Icon icon={faSidebarFlip} />
							</Button>
						</CursorTooltipTrigger>
					) : null}
				</CursorTooltipGroup>
			</div>
			<LoadingIndicator />
		</div>
	);
}

function InstanceSearchTrigger() {
	const [open, setOpen] = useState(false);

	useHotkey("Mod+K", () => setOpen(true));

	return (
		<>
			<CursorTooltipTrigger
				content={
					<>
						Search
						<CursorTooltipShortcut keys="K" />
					</>
				}
			>
				<Button
					variant="ghost"
					size="icon-sm"
					onClick={() => setOpen(true)}
					aria-label="Search"
				>
					<Icon icon={faMagnifyingGlass} />
				</Button>
			</CursorTooltipTrigger>
			<InstanceSearchDialog open={open} onOpenChange={setOpen} />
		</>
	);
}

function InstanceSearchDialog({
	open,
	onOpenChange,
}: {
	open: boolean;
	onOpenChange: (open: boolean) => void;
}) {
	const navigate = useNavigate();
	const queryClient = useQueryClient();
	const dataProvider = useDataProvider();

	const [value, setValue] = useState("");
	const [isPending, setIsPending] = useState(false);

	const goToActor = (actorId: string) => {
		void navigate({
			to: ".",
			search: (prev) => ({ ...prev, actorId }),
		});
	};

	const handleSubmit = async () => {
		const trimmed = value.trim();
		if (!trimmed) return;
		setIsPending(true);
		try {
			try {
				await queryClient.fetchQuery({
					...dataProvider.actorQueryOptions(trimmed),
					retry: false,
				});
				goToActor(trimmed);
				return;
			} catch {}

			// The input wasn't an instance ID, so treat it as a key. Keys are
			// only unique within a build, so every build name has to be probed.
			// Providers that can't list builds (e.g. the inspector) just leave
			// the lookup unresolved.
			const resolved = await (async () => {
				const builds = await queryClient.fetchInfiniteQuery(
					dataProvider.buildsQueryOptions(),
				);
				const buildNames = builds.pages.flatMap((page) =>
					Object.keys(page.names ?? {}),
				);

				return Promise.any(
					buildNames.map((name) =>
						queryClient.fetchQuery({
							...dataProvider.actorQueryOptions({
								key: trimmed,
								name,
							}),
							retry: false,
						}),
					),
				);
			})().catch(() => null);

			if (resolved) {
				goToActor(resolved.actorId);
				return;
			}

			toast.error(`No Actor found with ID or key "${trimmed}"`);
		} finally {
			setValue("");
			setIsPending(false);
			onOpenChange(false);
		}
	};

	return (
		<Dialog
			open={open}
			onOpenChange={(next) => {
				if (!next) setValue("");
				onOpenChange(next);
			}}
		>
			<DialogContent>
				<DialogHeader>
					<DialogTitle>Open Actor by ID</DialogTitle>
					<DialogDescription>
						Paste a full Actor instance ID or key to jump straight
						to it. Not a search — must match exactly.
					</DialogDescription>
				</DialogHeader>
				<div className="space-y-1.5">
					<label
						htmlFor="actor-lookup-input"
						className="text-xs font-medium text-muted-foreground"
					>
						Actor instance ID or key
					</label>
					<Input
						id="actor-lookup-input"
						type="text"
						value={value}
						placeholder="0193af8e-..."
						disabled={isPending}
						autoFocus
						autoComplete="off"
						spellCheck={false}
						className="font-mono-console text-xs"
						onChange={(e) => setValue(e.target.value)}
						onKeyDown={(e) => {
							if (e.key === "Enter") {
								e.preventDefault();
								void handleSubmit();
							}
						}}
					/>
				</div>
				<DialogFooter>
					<Button
						type="button"
						isLoading={isPending}
						disabled={!value.trim()}
						onClick={() => void handleSubmit()}
						startIcon={<Icon icon={faArrowUpRightFromSquare} />}
					>
						Open Actor
					</Button>
				</DialogFooter>
			</DialogContent>
		</Dialog>
	);
}

function LoadingIndicator() {
	const n = useSearch({
		from: "/_context",
		select: (state) => state.n,
	});

	const filters = useFiltersValue({ onlyStatic: true });
	const { isLoading } = useInfiniteQuery(
		useDataProvider().actorsListQueryOptions({ n, filters }),
	);
	if (isLoading) {
		return <ShimmerLine className="bottom-0" />;
	}
	return null;
}

function ActorsTableHeaderGate() {
	const { n } = useSearch({ from: "/_context" });
	const filters = useFiltersValue({ onlyStatic: true });
	const { data: actors = [] } = useInfiniteQuery(
		useDataProvider().actorsListQueryOptions({ n, filters }),
	);
	if (actors.length === 0) return null;
	return <ActorsListHeader />;
}

function List({
	viewportRef,
}: {
	viewportRef: RefObject<HTMLDivElement | null>;
}) {
	const filters = useFiltersValue({ onlyStatic: true });
	const { actorId, n } = useSearch({
		from: "/_context",
	});
	const { data: actors = [] } = useInfiniteQuery(
		useDataProvider().actorsListQueryOptions({ n, filters }),
	);

	const rowVirtualizer = useVirtualizer({
		count: actors.length,
		getScrollElement: () => viewportRef.current,
		estimateSize: () => 32,
		overscan: 5,
	});

	useEffect(() => {
		rowVirtualizer.measure();
	}, [rowVirtualizer]);

	return (
		<div
			className="relative w-full"
			style={{ height: rowVirtualizer.getTotalSize() }}
		>
			{rowVirtualizer.getVirtualItems().map((virtualItem) => {
				const actor = actors[virtualItem.index];
				return (
					<div
						key={actor.actorId}
						ref={rowVirtualizer.measureElement}
						data-index={virtualItem.index}
						className="absolute inset-x-0"
						style={{
							transform: `translateY(${virtualItem.start}px)`,
						}}
					>
						<ActorsListRow
							actorId={actor.actorId}
							isCurrent={actorId === actor.actorId}
						/>
					</div>
				);
			})}
		</div>
	);
}

function Pagination() {
	const n = useSearch({
		from: "/_context",
		select: (state) => state.n,
	});
	const filters = useFiltersValue({ onlyStatic: true });
	const { hasNextPage, isFetchingNextPage, fetchNextPage, data } =
		useSuspenseInfiniteQuery(
			useDataProvider().actorsListPaginationQueryOptions({
				n,
				filters,
			}),
		);

	if (isFetchingNextPage) {
		return <ListSkeleton />;
	}

	if (hasNextPage) {
		return <VisibilitySensor onChange={fetchNextPage} />;
	}

	return <EmptyState count={data || 0} />;
}

export function ListSkeleton() {
	return (
		<div className="w-full">
			{Array(RECORDS_PER_PAGE)
				.fill(null)
				.map((_, i) => (
					// biome-ignore lint/suspicious/noArrayIndexKey: skeleton loaders are static
					<ActorsListRowSkeleton key={i} />
				))}
		</div>
	);
}

const useRunnerConfigs = () => {
	const dataProvider = useDataProvider();
	const { data: runnerNamesCount = 0 } = useInfiniteQuery({
		...dataProvider.runnerNamesQueryOptions(),
		select: (data) => data.pages.flatMap((page) => page.names).length,
	});

	const { data: runnerConfigsCount = 0 } = useInfiniteQuery({
		...dataProvider.runnerConfigsQueryOptions(),
		select: (data) =>
			data.pages.flatMap((page) => Object.keys(page.runnerConfigs))
				.length,
	});

	return runnerConfigsCount + runnerNamesCount;
};

function EmptyState({ count }: { count: number }) {
	const navigate = useNavigate();
	const names = useSearch({
		from: "/_context",
		select: (state) => state.n,
	});
	const { copy } = useActorsView();
	const { remove, pick } = useActorsFilters();

	const dataProvider = useDataProvider();

	const { data: availableNamesCount = 0 } = useInfiniteQuery(
		dataProvider.buildsCountQueryOptions(),
	);

	const runnerConfigsCount = useRunnerConfigs();

	const filtersCount = useSearch({
		from: "/_context",
		select: (state) =>
			Object.values(pick(state, { onlyStatic: true })).length,
	});

	const clearFilters = () => {
		return navigate({
			to: ".",
			search: (prev) => ({
				...remove(prev || {}),
			}),
		});
	};

	return (
		<div className=" col-span-full my-4 flex flex-col items-center gap-2 justify-center">
			{(!names || names?.length === 0) && availableNamesCount > 0 ? (
				<div className="flex text-center text-foreground flex-1 justify-center items-center flex-col gap-2 my-12">
					<Icon icon={faQuestionSquare} className="text-4xl" />
					<p className="max-w-[400px]">
						No Actor Name selected.
						<br />
						<span className="text-sm text-muted-foreground">
							Select an Actor Name from the list on the left.
						</span>
					</p>
				</div>
			) : count === 0 ? (
				filtersCount === 0 ? (
					runnerConfigsCount === 0 ? (
						<div className="px-4">
							<NoProvidersAlert />
						</div>
					) : (
						<QuickstartEmptyState
							hasNameFilter={Boolean(names && names.length > 0)}
						/>
					)
				) : (
					<>
						<SmallText className="text-foreground text-center mt-8 mb-2">
							{copy.noActorsMatchFilter}
						</SmallText>
						<Button variant="outline" mx="4" onClick={clearFilters}>
							Clear filter
						</Button>
					</>
				)
			) : count > RECORDS_PER_PAGE ? (
				// Only worth saying once the user has actually scrolled through
				// more than one page; for short lists it is just noise.
				<SmallText className="text-muted-foreground text-center text-xs">
					{copy.noMoreActors}
				</SmallText>
			) : null}
		</div>
	);
}

function useFiltersChangeCallback(): OnFiltersChange {
	const navigate = useNavigate();
	const { pick, remove } = useActorsFilters();
	const [value, setLs] = useLocalStorage(
		ls.actorsEphemeralFilters.key,
		() => ({ wakeOnSelect: { value: ["1"] } }),
		{
			deserializer: (value) => JSON.parse(value),
			serializer: (value) => JSON.stringify(value),
		},
	);

	return useCallback(
		(fnOrValue) => {
			if (typeof fnOrValue === "function") {
				navigate({
					to: ".",
					search: (old) => {
						const filters = pick(old || {}, { onlyStatic: true });
						const prev = remove(old || {});

						return {
							...prev,
							...Object.fromEntries(
								Object.entries(
									pick(fnOrValue(filters), {
										onlyStatic: true,
									}),
								).filter(
									([, filter]) => filter.value.length > 0,
								),
							),
						};
					},
				});

				setLs(pick(fnOrValue(value), { onlyEphemeral: true }));
			} else {
				navigate({
					to: ".",
					search: (value) => ({
						...remove(value || {}),
						...Object.fromEntries(
							Object.entries(
								pick(fnOrValue, { onlyStatic: true }),
							).filter(([, filter]) => filter.value.length > 0),
						),
					}),
				});
				setLs(pick(fnOrValue || {}, { onlyEphemeral: true }));
			}
		},
		[navigate, pick, remove, setLs, value],
	);
}

function Display() {
	const { definitions } = useActorsFilters();
	const filters = useFiltersValue();
	const onFiltersChange = useFiltersChangeCallback();

	return (
		<FiltersDisplay
			value={filters}
			definitions={definitions}
			onChange={onFiltersChange}
		/>
	);
}

const QUICKSTART_DEFINE_SNIPPET = `import { actor, setup } from "rivetkit";

const counter = actor({
  state: { count: 0 },
  actions: {
    increment: (c, by: number) => {
      c.state.count += by;
      return c.state.count;
    },
  },
});

export const registry = setup({ use: { counter } });`;

function QuickstartEmptyState({ hasNameFilter }: { hasNameFilter: boolean }) {
	const { links } = useActorsView();
	const title = hasNameFilter
		? "No actors created"
		: "Get started with Actors";
	const description = hasNameFilter
		? "Actor instances appear here once created."
		: "Define an actor in your registry. Instances appear here as your code calls them.";

	return (
		<div className="mx-auto my-10 w-full max-w-3xl px-4">
			<div className="rounded-2xl border border-foreground/10 bg-card shadow-sm overflow-hidden">
				<div className="px-8 pt-8 pb-6 border-b border-foreground/10">
					<h2 className="text-xl font-semibold tracking-tight">
						{title}
					</h2>
					<p className="mt-1.5 text-sm text-muted-foreground leading-relaxed">
						{description}
					</p>
				</div>

				{hasNameFilter ? null : (
					<div className="px-8 py-6">
						<QuickstartSection
							step={1}
							title="Define an actor in your registry"
							code={QUICKSTART_DEFINE_SNIPPET}
						/>
					</div>
				)}

				<div className="px-8 py-4 border-t border-foreground/10 flex flex-wrap items-center justify-between gap-6">
					<a
						href="https://www.rivet.dev/docs/actors/quickstart/"
						target="_blank"
						rel="noopener noreferrer"
						className="group inline-flex items-center gap-2 text-sm font-medium text-foreground hover:text-foreground/80"
					>
						<Icon icon={faBookOpen} className="size-3.5" />
						Quickstart
						<Icon
							icon={faArrowUpRightFromSquare}
							className="size-3 text-muted-foreground opacity-0 group-hover:opacity-100 transition-opacity"
						/>
					</a>
					<CreateActorButton
						variant="default"
						label={hasNameFilter ? "Create Actor" : undefined}
					/>
				</div>
			</div>
		</div>
	);
}

function QuickstartSection({
	step,
	title,
	code,
}: {
	step: number;
	title: string;
	code: string;
}) {
	return (
		<div className="flex gap-4">
			<div className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-foreground/10 text-xs font-semibold text-foreground">
				{step}
			</div>
			<div className="flex-1 min-w-0">
				<p className="text-sm font-medium text-foreground mb-2">
					{title}
				</p>
				<CodePreview
					code={code}
					language="typescript"
					className="rounded-lg border border-foreground/10 bg-background/60 px-4 py-3 text-xs leading-relaxed overflow-x-auto font-mono [&_pre]:!bg-transparent"
				/>
			</div>
		</div>
	);
}
