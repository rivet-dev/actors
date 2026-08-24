import { faCircleInfo, faTrash, Icon } from "@rivet-gg/icons";
import { useInfiniteQuery } from "@tanstack/react-query";
import type { ReactNode } from "react";
import {
	type FieldArrayPath,
	type FieldPath,
	type Path,
	type PathValue,
	type UseFormReturn,
	useFieldArray,
	useFormContext,
} from "react-hook-form";
import z from "zod";
import { EndpointHealthIndicator } from "@/app/forms/serverless-endpoint-health";
import { RunnerConfigToggleGroup } from "@/app/runner-config-toggle-group";
import {
	Button,
	Checkbox,
	createSchemaForm,
	FormControl,
	FormDescription,
	FormField,
	FormFieldContext,
	FormItem,
	FormLabel,
	FormMessage,
	Input,
	Label,
	Switch,
} from "@/components";
import { ActorRegion, useEngineCompatDataProvider } from "@/components/actors";
import { WithTooltip } from "@/components/ui/tooltip";
import { VisibilitySensor } from "@/components/visibility-sensor";

const LabelWithInfo = ({
	children,
	info,
}: {
	children: ReactNode;
	info: ReactNode;
}) => (
	<div className="flex items-center gap-1.5">
		<FormLabel>{children}</FormLabel>
		<FormDescription className="sr-only">{info}</FormDescription>
		<WithTooltip
			content={info}
			delayDuration={100}
			contentProps={{ className: "z-[80]", sideOffset: 6 }}
			trigger={
				<button
					type="button"
					tabIndex={-1}
					className="inline-flex p-0.5 -m-0.5 text-muted-foreground/60 hover:text-muted-foreground transition-colors cursor-help"
					aria-label="More info"
				>
					<Icon icon={faCircleInfo} className="h-3.5 w-3.5" />
				</button>
			}
		/>
	</div>
);

export const runtimeModeSchema = z.enum(["serverless", "serverful"]);
export type RuntimeMode = z.infer<typeof runtimeModeSchema>;

export const baseFormSchema = z.object({
	mode: runtimeModeSchema.default("serverless"),
	url: z.string().optional().default(""),
	maxRunners: z.coerce.number().positive().optional(),
	minRunners: z.coerce.number().min(0).optional(),
	requestLifespan: z.coerce.number().positive().optional(),
	runnersMargin: z.coerce.number().min(0).optional(),
	slotsPerRunner: z.coerce.number().positive().optional(),
	maxConcurrentActors: z.coerce.number().positive().optional(),
	drainGracePeriod: z.coerce.number().min(0).optional(),
	autoUpgrade: z.boolean().optional().default(false),
	headers: z.array(z.array(z.string())).default([]),
	regions: z
		.record(z.string(), z.boolean().optional())
		.optional()
		.refine((obj) => {
			return Object.values(obj || {}).some((v) => v);
		}, "At least one region must be selected."),
});

export function validateRuntimeModeFields(
	data: { mode?: RuntimeMode; url?: string },
	ctx: z.RefinementCtx,
	pathPrefix: (string | number)[] = [],
) {
	if (data.mode === "serverless") {
		if (!data.url || !z.string().url().safeParse(data.url).success) {
			ctx.addIssue({
				path: [...pathPrefix, "url"],
				code: z.ZodIssueCode.custom,
				message: "Please enter a valid URL.",
			});
		}
	}
}

export const formSchema = baseFormSchema.superRefine((data, ctx) => {
	validateRuntimeModeFields(data, ctx);
});

export type FormValues = z.infer<typeof formSchema>;
export type SubmitHandler = (
	values: FormValues,
	form: UseFormReturn<FormValues>,
) => Promise<void>;

const { Form, Submit, SetValue } = createSchemaForm(formSchema);
export { Form, Submit, SetValue };

export const Mode = <TValues extends Record<string, any> = FormValues>({
	name = "mode" as FieldPath<TValues>,
	className,
}: {
	name?: FieldPath<TValues>;
	className?: string;
}) => {
	const { control } = useFormContext<TValues>();
	return (
		<FormField
			control={control}
			name={name}
			render={({ field }) => (
				<RunnerConfigToggleGroup
					mode={(field.value as string) || "serverless"}
					onChange={field.onChange}
					className={className ?? "mb-2"}
				/>
			)}
		/>
	);
};

export const Url = <TValues extends Record<string, any> = FormValues>({
	name = "url" as FieldPath<TValues>,
	headersName,
	enabledName,
	className,
}: {
	name?: FieldPath<TValues>;
	headersName?: string;
	enabledName?: string;
	className?: string;
}) => {
	const { control } = useFormContext<TValues>();
	return (
		<FormField
			control={control}
			name={name}
			render={({ field }) => (
				<FormItem className={className}>
					<FormLabel className="col-span-1">Endpoint</FormLabel>
					<FormControl className="row-start-2">
						<div className="relative">
							<Input
								placeholder="https://your-rivet-runner"
								className="pr-10"
								{...field}
							/>
							<EndpointHealthIndicator
								endpointName={name}
								headersName={headersName}
								enabledName={enabledName}
							/>
						</div>
					</FormControl>
					<FormMessage className="col-span-1" />
				</FormItem>
			)}
		/>
	);
};

export const MinRunners = <TValues extends Record<string, any> = FormValues>({
	name = "minRunners" as FieldPath<TValues>,
	className,
}: {
	name?: FieldPath<TValues>;
	className?: string;
}) => {
	const { control } = useFormContext<TValues>();
	return (
		<FormField
			control={control}
			name={name}
			render={({ field }) => (
				<FormItem className={className}>
					<LabelWithInfo info="The minimum number of runners to keep running.">
						Min Runners
					</LabelWithInfo>
					<FormControl className="row-start-2">
						<Input type="number" {...field} />
					</FormControl>
					<FormMessage className="col-span-1" />
				</FormItem>
			)}
		/>
	);
};

export const MaxRunners = <TValues extends Record<string, any> = FormValues>({
	name = "maxRunners" as FieldPath<TValues>,
	className,
}: {
	name?: FieldPath<TValues>;
	className?: string;
}) => {
	const { control } = useFormContext<TValues>();
	return (
		<FormField
			control={control}
			name={name}
			render={({ field }) => (
				<FormItem className={className}>
					<LabelWithInfo info="The maximum number of runners that can be created to handle load.">
						Max Runners
					</LabelWithInfo>
					<FormControl className="row-start-2">
						<Input type="number" {...field} />
					</FormControl>
					<FormMessage className="col-span-1" />
				</FormItem>
			)}
		/>
	);
};

export const RequestLifespan = <
	TValues extends Record<string, any> = FormValues,
>({
	name = "requestLifespan" as FieldPath<TValues>,
	className,
}: {
	name?: FieldPath<TValues>;
	className?: string;
}) => {
	const { control } = useFormContext<TValues>();
	return (
		<FormField
			control={control}
			name={name}
			render={({ field }) => (
				<FormItem className={className}>
					<LabelWithInfo info="The maximum duration (in seconds) a request can take before being terminated.">
						Request Lifespan
					</LabelWithInfo>
					<FormControl className="row-start-2">
						<Input type="number" {...field} />
					</FormControl>
					<FormMessage className="col-span-1" />
				</FormItem>
			)}
		/>
	);
};

export const RunnersMargin = <
	TValues extends Record<string, any> = FormValues,
>({
	name = "runnersMargin" as FieldPath<TValues>,
	className,
}: {
	name?: FieldPath<TValues>;
	className?: string;
}) => {
	const { control } = useFormContext<TValues>();
	return (
		<FormField
			control={control}
			name={name}
			render={({ field }) => (
				<FormItem className={className}>
					<LabelWithInfo info="The number of extra runners to keep running to handle sudden spikes in load.">
						Runners Margin
					</LabelWithInfo>
					<FormControl className="row-start-2">
						<Input type="number" {...field} />
					</FormControl>
					<FormMessage className="col-span-1" />
				</FormItem>
			)}
		/>
	);
};

export const SlotsPerRunner = <
	TValues extends Record<string, any> = FormValues,
>({
	name = "slotsPerRunner" as FieldPath<TValues>,
	className,
}: {
	name?: FieldPath<TValues>;
	className?: string;
}) => {
	const { control } = useFormContext<TValues>();
	return (
		<FormField
			control={control}
			name={name}
			render={({ field }) => (
				<FormItem className={className}>
					<LabelWithInfo info="The number of concurrent slots each runner can handle.">
						Slots Per Runner
					</LabelWithInfo>
					<FormControl className="row-start-2">
						<Input type="number" {...field} />
					</FormControl>
					<FormMessage className="col-span-1" />
				</FormItem>
			)}
		/>
	);
};

export const Headers = <TValues extends Record<string, any> = FormValues>({
	name = "headers" as FieldArrayPath<TValues>,
}: {
	name?: FieldArrayPath<TValues>;
}) => {
	const { control, setValue, watch } = useFormContext<TValues>();
	const { fields, append, remove } = useFieldArray<TValues>({
		name,
		control,
	});

	return (
		<div className="space-y-3">
			<div className="space-y-1">
				<h3 className="text-sm font-medium leading-none text-foreground">
					Custom headers
				</h3>
				<FormDescription className="text-xs">
					Headers added to each request to the runner. Useful for
					providing authentication or other information.
				</FormDescription>
			</div>
			<div className="grid grid-cols-[1fr,1fr,auto] grid-rows-[repeat(3,auto)] items-start gap-2 empty:hidden">
				{fields.length > 0 ? (
					<>
						<Label asChild>
							<p>Name</p>
						</Label>
						<Label asChild>
							<p>Value</p>
						</Label>
						<p></p>
					</>
				) : null}
				{fields.map((field, index) => (
					<div
						key={field.id}
						className="grid grid-cols-subgrid grid-rows-
col-span-full flex-1"
					>
						<FormFieldContext.Provider
							value={{ name: `${name}.${index}.0` }}
						>
							<FormItem
								flex="1"
								className="grid grid-cols-subgrid grid-rows-subgrid row-span-full"
							>
								<FormLabel aria-hidden hidden>
									Key
								</FormLabel>
								<FormControl>
									<Input
										placeholder="Enter a value"
										className="w-full"
										value={watch(
											`${name}.${index}.0` as Path<TValues>,
										)}
										onChange={(e) => {
											setValue(
												`${name}.${index}.0` as Path<TValues>,
												e.target.value as PathValue<
													TValues,
													Path<TValues>
												>,
												{
													shouldDirty: true,
													shouldTouch: true,
													shouldValidate: true,
												},
											);
										}}
									/>
								</FormControl>
								<FormMessage />
							</FormItem>
						</FormFieldContext.Provider>

						<FormFieldContext.Provider
							value={{ name: `${name}.${index}.1` }}
						>
							<FormItem
								flex="1"
								className="grid grid-cols-subgrid grid-rows-subgrid row-span-full"
							>
								<FormLabel aria-hidden hidden>
									Value
								</FormLabel>
								<FormControl>
									<Input
										placeholder="Enter a value"
										className="w-full"
										value={watch(
											`${name}.${index}.1` as Path<TValues>,
										)}
										onChange={(e) => {
											setValue(
												`${name}.${index}.1` as Path<TValues>,
												e.target.value as PathValue<
													TValues,
													Path<TValues>
												>,
												{
													shouldDirty: true,
													shouldTouch: true,
													shouldValidate: true,
												},
											);
										}}
									/>
								</FormControl>
								<FormMessage />
							</FormItem>
						</FormFieldContext.Provider>
						<Button
							size="icon"
							className="self-end row-start-1"
							variant="secondary"
							type="button"
							onClick={() => remove(index)}
						>
							<Icon icon={faTrash} />
						</Button>
					</div>
				))}
			</div>
			<Button
				className="justify-self-start"
				variant="secondary"
				size="sm"
				type="button"
				onClick={() =>
					append([["", ""]] as PathValue<TValues, Path<TValues>>)
				}
			>
				Add a header
			</Button>
		</div>
	);
};

export const MaxConcurrentActors = <
	TValues extends Record<string, any> = FormValues,
>({
	name = "maxConcurrentActors" as FieldPath<TValues>,
	className,
}: {
	name?: FieldPath<TValues>;
	className?: string;
}) => {
	const { control } = useFormContext<TValues>();
	return (
		<FormField
			control={control}
			name={name}
			render={({ field }) => (
				<FormItem className={className}>
					<LabelWithInfo info="Maximum actors allowed to run concurrently per runner.">
						Max Concurrent Actors
					</LabelWithInfo>
					<FormControl className="row-start-2">
						<Input
							type="number"
							placeholder="Unlimited"
							{...field}
							value={field.value ?? ""}
						/>
					</FormControl>
					<FormMessage className="col-span-1" />
				</FormItem>
			)}
		/>
	);
};

export const DrainGracePeriod = <
	TValues extends Record<string, any> = FormValues,
>({
	name = "drainGracePeriod" as FieldPath<TValues>,
	className,
}: {
	name?: FieldPath<TValues>;
	className?: string;
}) => {
	const { control } = useFormContext<TValues>();
	return (
		<FormField
			control={control}
			name={name}
			render={({ field }) => (
				<FormItem className={className}>
					<LabelWithInfo info="Time to wait for actors to finish before forcefully stopping.">
						Drain Grace Period (s)
					</LabelWithInfo>
					<FormControl className="row-start-2">
						<Input
							type="number"
							placeholder="10"
							{...field}
							value={field.value ?? ""}
						/>
					</FormControl>
					<FormMessage className="col-span-1" />
				</FormItem>
			)}
		/>
	);
};

export const AutoUpgrade = <TValues extends Record<string, any> = FormValues>({
	name = "autoUpgrade" as FieldPath<TValues>,
}: {
	name?: FieldPath<TValues>;
}) => {
	const { control } = useFormContext<TValues>();
	return (
		<FormField
			control={control}
			name={name}
			render={({ field }) => (
				<FormItem className="flex flex-row items-center gap-3">
					<FormControl>
						<Switch
							checked={field.value ?? false}
							onCheckedChange={field.onChange}
						/>
					</FormControl>
					<div>
						<FormLabel>Auto Upgrade Actors</FormLabel>
						<FormDescription>
							Automatically upgrade actors when a new runner
							version is available.
						</FormDescription>
					</div>
					<FormMessage />
				</FormItem>
			)}
		/>
	);
};

export const Regions = () => {
	const { control } = useFormContext<FormValues>();
	const { data, hasNextPage, fetchNextPage, isFetchingNextPage } =
		useInfiniteQuery({
			...useEngineCompatDataProvider().datacentersQueryOptions(),
			maxPages: Infinity,
		});

	return (
		<div className="space-y-3">
			<div className="space-y-1">
				<h3 className="text-sm font-medium leading-none text-foreground">
					Datacenters
				</h3>
				<FormDescription className="text-xs">
					Datacenters where this provider can deploy actors.
				</FormDescription>
			</div>
			<div className="space-y-3">
				{data?.map((region) => (
					<FormField
						key={region.name}
						control={control}
						name={`regions.${region.name}`}
						render={({ field }) => (
							<>
								<div className="flex items-start gap-3">
									<Checkbox
										id={`region-${region.name}`}
										checked={field.value ?? false}
										name={field.name}
										onCheckedChange={field.onChange}
									/>
									<div className="grid gap-2">
										<Label
											htmlFor={`region-${region.name}`}
										>
											<ActorRegion
												regionId={region.name}
												showLabel
											/>
										</Label>
									</div>
								</div>
								<FormMessage />
							</>
						)}
					/>
				))}
				{hasNextPage && !isFetchingNextPage ? (
					<VisibilitySensor onChange={fetchNextPage} />
				) : null}
			</div>{" "}
			<FormField
				control={control}
				name="regions"
				render={() => <FormMessage />}
			/>
		</div>
	);
};
