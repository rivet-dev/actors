import { type Rivet } from "@rivetkit/engine-api-full";
import {
	infiniteQueryOptions,
	useInfiniteQuery,
	useSuspenseInfiniteQuery,
} from "@tanstack/react-query";
import { useEffect, useRef } from "react";
import { type UseFormReturn, useFormContext } from "react-hook-form";
import z from "zod";
import { CodePreview, Combobox, Input, Label } from "@/components";
import { JsonCode } from "../../code-mirror";
import { createSchemaForm } from "../../lib/create-schema-form";
import {
	FormControl,
	FormDescription,
	FormField,
	FormItem,
	FormLabel,
	FormMessage,
} from "../../ui/form";
import { AllRunnerSelect } from "../all-runner-select";
import { BuildSelect } from "../build-select";
import { useEngineCompatDataProvider } from "../data-provider";
import { RegionSelect } from "../region-select";

const jsonValid = z.custom<string>(
	(value) => {
		if (value.trim() === "") return true;
		try {
			JSON.parse(value);
			return true;
		} catch {
			return false;
		}
	},
	{ fatal: true, message: "Must be valid JSON" },
);

const emptyRunnerNamesQueryOptions = infiniteQueryOptions({
	queryKey: ["noop-runner-names"] as readonly unknown[],
	queryFn: async (): Promise<Rivet.RunnersListNamesResponse> => ({
		names: [],
		pagination: {},
	}),
	initialPageParam: undefined as string | undefined,
	getNextPageParam: () => undefined,
	select: (data) => data.pages.flatMap((page) => page.names),
});

export const formSchema = z
	.object({
		name: z.string().nonempty("Build is required"),
		// regionId: z.string(),
		key: z.string(),
		input: jsonValid.optional(),
		// tags: tagsFormSchema.shape.tags,

		datacenter: z.string(),
		runnerNameSelector: z.string(),
	})
	.partial({ datacenter: true, runnerNameSelector: true });

export type FormValues = z.infer<typeof formSchema>;
export type SubmitHandler = (
	values: FormValues,
	form: UseFormReturn<FormValues>,
) => Promise<void>;

const { Form, Submit } = createSchemaForm(formSchema);
export { Form, Submit };

export const Build = () => {
	const { control } = useFormContext<FormValues>();
	return (
		<FormField
			control={control}
			name="name"
			render={({ field }) => (
				<FormItem>
					<FormLabel>Name</FormLabel>
					<FormControl>
						<BuildSelect
							onValueChange={field.onChange}
							value={field.value}
						/>
					</FormControl>
					<FormDescription>
						Used to differentiate between different actor types.
					</FormDescription>
					<FormMessage />
				</FormItem>
			)}
		/>
	);
};

export const Keys = () => {
	const { control } = useFormContext<FormValues>();
	return (
		<FormField
			control={control}
			name="key"
			render={({ field }) => (
				<FormItem>
					<FormLabel>Key</FormLabel>
					<FormControl>
						<Input {...field} className="font-mono-console" />
					</FormControl>
					<FormDescription>Identifier for the Actor.</FormDescription>
					<FormMessage />
				</FormItem>
			)}
		/>
	);
};

export const JsonInput = () => {
	const { control } = useFormContext<FormValues>();
	return (
		<FormField
			control={control}
			name="input"
			render={({ field }) => (
				<FormItem>
					<FormLabel>Input</FormLabel>
					<FormControl>
						<JsonCode
							minHeight="5rem"
							onChange={field.onChange}
							value={field.value}
						/>
					</FormControl>
					<FormDescription>
						Optional JSON object that will be passed to the Actor as
						input.
					</FormDescription>
					<FormMessage />
				</FormItem>
			)}
		/>
	);
};

export const RunnerNameSelector = () => {
	const { control } = useFormContext<FormValues>();

	return (
		<FormField
			control={control}
			name="runnerNameSelector"
			render={({ field }) => (
				<FormItem>
					<FormLabel>Runner</FormLabel>
					<FormControl>
						<AllRunnerSelect
							onValueChange={field.onChange}
							value={field.value || ""}
						/>
					</FormControl>
					<FormDescription>
						Runner name selector for the actor. This is used to
						select which runner the actor will run on.
					</FormDescription>
					<FormMessage />
				</FormItem>
			)}
		/>
	);
};

const selectRunnerConfigKeys = (data: {
	pages: { runnerConfigs: Record<string, unknown> }[];
}) => data.pages.flatMap((page) => Object.keys(page.runnerConfigs));

const emptyRunnerConfigKeysQueryOptions = infiniteQueryOptions({
	queryKey: ["noop-runner-config-keys"] as readonly unknown[],
	queryFn: async (): Promise<Rivet.RunnerConfigsListResponse> => ({
		runnerConfigs: {},
		pagination: {},
	}),
	initialPageParam: undefined as string | undefined,
	getNextPageParam: () => undefined,
	select: selectRunnerConfigKeys,
});

// The pool the create form defaults to: the runner config named "default" when
// present, otherwise the first one. Undefined when there are no runner configs.
function resolveDefaultPool(keys: string[]): string | undefined {
	if (keys.length === 0) return undefined;
	return keys.includes("default") ? "default" : keys[0];
}

// Loads the namespace's runner-config keys (the pool names) and the resolved
// default pool. Pages are fetched lazily via `fetchNextPage` (the Pool combobox
// loads more on scroll); empty on providers without runner configs.
function useRunnerConfigKeys() {
	const dataProvider = useEngineCompatDataProvider();
	const hasRunnerConfigs = "runnerConfigsQueryOptions" in dataProvider;
	const {
		data: keys = [],
		hasNextPage,
		isLoading,
		isFetchingNextPage,
		fetchNextPage,
	} = useInfiniteQuery<
		Rivet.RunnerConfigsListResponse,
		Error,
		string[],
		readonly unknown[],
		string | undefined
	>({
		...(hasRunnerConfigs
			? {
					...dataProvider.runnerConfigsQueryOptions(),
					select: selectRunnerConfigKeys,
				}
			: emptyRunnerConfigKeysQueryOptions),
		enabled: hasRunnerConfigs,
	});
	return {
		keys,
		defaultPool: resolveDefaultPool(keys),
		hasNextPage,
		isLoading,
		isFetchingNextPage,
		fetchNextPage,
	};
}

// Lets the dialogs submit the resolved default pool even when Advanced (where
// the Pool selector lives) is never opened. Falls back to "default".
export function useDefaultRunnerNameSelector(): string {
	return useRunnerConfigKeys().defaultPool ?? "default";
}

// Pool selector bound to `runnerNameSelector`. Hidden unless there is more than
// one pool to pick between; the submit handler still sends the resolved default.
export const Pool = () => {
	const { control } = useFormContext<FormValues>();
	const {
		keys,
		defaultPool,
		hasNextPage,
		isLoading,
		isFetchingNextPage,
		fetchNextPage,
	} = useRunnerConfigKeys();

	if (keys.length <= 1) {
		return null;
	}

	const options = keys.map((key) => ({ label: key, value: key }));

	return (
		<FormField
			control={control}
			name="runnerNameSelector"
			render={({ field }) => (
				<FormItem>
					<FormLabel>Pool</FormLabel>
					<FormControl>
						<Combobox
							placeholder="Choose a pool..."
							options={options}
							value={field.value || defaultPool || ""}
							onValueChange={field.onChange}
							className="w-full"
							isLoading={isLoading || isFetchingNextPage}
							onLoadMore={hasNextPage ? fetchNextPage : undefined}
						/>
					</FormControl>
					<FormDescription>
						The pool the Actor will run on.
					</FormDescription>
					<FormMessage />
				</FormItem>
			)}
		/>
	);
};

export const ActorPreview = () => {
	const { watch } = useFormContext<FormValues>();

	const [name, key] = watch(["name", "key"]);

	return (
		<div className="space-y-2">
			<Label>Code</Label>
			<div className="text-xs border rounded-md p-2">
				<CodePreview
					code={`client.${name}.getOrCreate(${JSON.stringify(key)})`}
					language="typescript"
				/>
			</div>
			<p className={"text-sm text-muted-foreground"}>
				You can use above code snippet to get or create the actor in
				your application. For more information, see the{" "}
				<a
					href="https://www.rivet.dev/docs/clients"
					target="_blank"
					rel="noopener noreferrer"
					className="underline"
				>
					documentation
				</a>
				.
			</p>
		</div>
	);
};

export const PrefillActorName = () => {
	const prefilled = useRef(false);
	const { watch } = useFormContext<FormValues>();

	const { data: name, isSuccess } = useInfiniteQuery({
		...useEngineCompatDataProvider().buildsQueryOptions(),
		select: (data) => Object.keys(data.pages[0].names)[0],
	});

	const watchedValue = watch("name");

	const { setValue } = useFormContext<FormValues>();

	useEffect(() => {
		if (name && isSuccess && !watchedValue && !prefilled.current) {
			setValue("name", name);
			prefilled.current = true;
		}
	}, [name, setValue, isSuccess, watchedValue]);

	return null;
};

export const PrefillRunnerName = () => {
	const prefilled = useRef(false);
	const { watch } = useFormContext<FormValues>();
	const dataProvider = useEngineCompatDataProvider();
	const hasRunnerNames = "runnerNamesQueryOptions" in dataProvider;

	const { data = [], isSuccess } = useInfiniteQuery<
		Rivet.RunnersListNamesResponse,
		Error,
		string[],
		readonly unknown[],
		string | undefined
	>({
		...(hasRunnerNames
			? dataProvider.runnerNamesQueryOptions()
			: emptyRunnerNamesQueryOptions),
		enabled: hasRunnerNames,
	});

	const watchedValue = watch("runnerNameSelector");

	const { setValue } = useFormContext<FormValues>();

	useEffect(() => {
		if (
			data.length > 0 &&
			isSuccess &&
			!watchedValue &&
			!prefilled.current
		) {
			setValue("runnerNameSelector", data[0]);
			prefilled.current = true;
		}
	}, [data, setValue, isSuccess, watchedValue]);

	return null;
};

export const PrefillDatacenter = () => {
	const prefilled = useRef(false);
	const { watch } = useFormContext<FormValues>();

	const { data: datacenter, isSuccess } = useSuspenseInfiniteQuery({
		...useEngineCompatDataProvider().runnerConfigsQueryOptions(),
		select: (data) => {
			return Object.keys(
				Object.values(data.pages[0].runnerConfigs || {})?.[0]
					?.datacenters || {},
			)?.[0];
		},
	});

	const watchedValue = watch("datacenter");

	const { setValue } = useFormContext<FormValues>();

	useEffect(() => {
		if (datacenter && isSuccess && !watchedValue && !prefilled.current) {
			setValue("datacenter", datacenter);
			prefilled.current = true;
		}
	}, [datacenter, setValue, isSuccess, watchedValue]);

	return null;
};

export const Datacenter = () => {
	const { control } = useFormContext<FormValues>();

	return (
		<FormField
			control={control}
			name="datacenter"
			render={({ field }) => (
				<FormItem>
					<FormLabel>Datacenter</FormLabel>
					<FormControl>
						<RegionSelect
							showAuto={false}
							value={field.value}
							onValueChange={field.onChange}
						/>
					</FormControl>
					<FormDescription>
						The datacenter where the Actor will be deployed.
					</FormDescription>
					<FormMessage />
				</FormItem>
			)}
		/>
	);
};

// export const Tags = () => {
// 	// const setValues = useSetAtom(actorCustomTagValues);
// 	// const setKeys = useSetAtom(actorCustomTagKeys);

// 	const { data: tags = [] } = useInfiniteQuery(
// 		useManagerQueries().actorsTagsQueryOptions(),
// 	);

// 	const keys = useMemo(() => {
// 		return Array.from(
// 			new Set(tags.flatMap((tag) => Object.keys(tag))),
// 		).sort();
// 	}, [tags]);
// 	const values = useMemo(() => {
// 		return Array.from(
// 			new Set(tags.flatMap((tag) => Object.values(tag))),
// 		).sort();
// 	}, [tags]);

// 	return (
// 		<div className="space-y-2">
// 			<Label>Tags</Label>
// 			<TagsInput
// 				keys={keys.map((key) => ({
// 					label: key,
// 					value: key,
// 				}))}
// 				values={values.map((value) => ({
// 					label: value,
// 					value: value,
// 				}))}
// 				onCreateKeyOption={(value) => {
// 					// setKeys((old) =>
// 					// 	Array.from(new Set([...old, value]).values()),
// 					// );
// 				}}
// 				onCreateValueOption={(value) => {
// 					// setValues((old) =>
// 					// 	Array.from(new Set([...old, value]).values()),
// 					// );
// 				}}
// 			/>
// 		</div>
// 	);
// };
