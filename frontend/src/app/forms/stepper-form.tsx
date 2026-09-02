import { zodResolver } from "@hookform/resolvers/zod";
import {
	faArrowLeft,
	faArrowRight,
	faTriangleExclamation,
	Icon,
} from "@rivet-gg/icons";
import type * as Stepperize from "@stepperize/react";
import { AnimatePresence, motion } from "framer-motion";
import {
	createContext,
	type MutableRefObject,
	type ReactNode,
	useContext,
	useLayoutEffect,
	useRef,
	useState,
} from "react";
import {
	FormProvider,
	type UseFormProps,
	type UseFormReturn,
	useForm,
	useFormContext,
	useWatch,
} from "react-hook-form";
import type * as z from "zod";
import { Button, cn } from "@/components";
import type { defineStepper } from "@/components/ui/stepper";
import { posthog } from "@/lib/posthog";

export type StepConfirm<TValues = Record<string, unknown>> = (
	values: TValues,
) => ReactNode | null | Promise<ReactNode | null>;

type Step = Stepperize.Step & {
	assist?: boolean;
	// Title/description may be static or derived from the live form values so a
	// step can adapt its heading to earlier choices (e.g. the selected platform).
	description?: string | ((values: Record<string, unknown>) => string);
	titleFor?: (values: Record<string, unknown>) => string;
	schema: z.ZodSchema | ((values: Record<string, unknown>) => z.ZodSchema);
	next?: string;
	previous?: string;
	showNext?: boolean;
	showPrevious?: boolean;
	group?: string;
	isVisible?: (values: Record<string, unknown>) => boolean;
	// method-style declaration so consumers can supply a narrower values type
	// (parameter contravariance would otherwise reject typed callbacks).
	confirm?(
		values: Record<string, unknown>,
	): ReactNode | null | Promise<ReactNode | null>;
};

type StepVisibilityContextType = {
	isStepVisible: (stepId: string) => boolean;
	visibleStepIndex: (stepId: string) => number;
	visibleStepCount: number;
};

export const StepVisibilityContext = createContext<StepVisibilityContextType>({
	isStepVisible: () => true,
	visibleStepIndex: () => 0,
	visibleStepCount: 1,
});

type StepperProps<Steps extends Step[]> = ReturnType<
	typeof defineStepper<Steps>
>;

type StepperFormContextType = {
	submitForm: (() => void) | null;
};

const StepperFormContext = createContext<StepperFormContextType>({
	submitForm: null,
});

export const useStepperFormSubmit = () => {
	const context = useContext(StepperFormContext);
	return context.submitForm;
};

export type JoinStepSchemas<T extends Step[]> = T extends [
	infer First,
	...infer Rest,
]
	? First extends Step
		? First["schema"] extends z.ZodSchema
			? Rest extends Step[]
				? z.ZodIntersection<
						First["schema"],
						JoinStepSchemas<Rest> extends z.ZodTypeAny
							? JoinStepSchemas<Rest>
							: // biome-ignore lint/complexity/noBannedTypes: required for zod generic
								z.ZodObject<{}>
					>
				: First["schema"]
			: First["schema"] extends (
						values: Record<string, unknown>,
					) => z.ZodSchema
				? ReturnType<First["schema"]>
				: // biome-ignore lint/complexity/noBannedTypes: required for zod generic
					z.ZodObject<{}>
		: // biome-ignore lint/complexity/noBannedTypes: required for zod generic
			z.ZodObject<{}>
	: // biome-ignore lint/complexity/noBannedTypes: required for zod generic
		z.ZodObject<{}>;

type StepperFormProps<Steps extends Step[]> = StepperProps<Steps> &
	UseFormProps<z.infer<JoinStepSchemas<Steps>>> & {
		onSubmit?: (opts: {
			values: z.infer<JoinStepSchemas<Steps>>;
			form: UseFormReturn<z.infer<JoinStepSchemas<Steps>>>;
			stepper: ReturnType<Stepperize.StepperReturn<Steps>["useStepper"]>;
		}) => Promise<void> | void;
		onPartialSubmit?: (opts: {
			values: z.infer<JoinStepSchemas<Steps>>;
			form: UseFormReturn<z.infer<JoinStepSchemas<Steps>>>;
			stepper: ReturnType<Stepperize.StepperReturn<Steps>["useStepper"]>;
		}) => Promise<void> | void;
		content: Record<Steps[number]["id"], () => ReactNode>;
		showAllSteps?: boolean;
		singlePage?: boolean;
		initialStep?: Steps[number]["id"];
		controls?: ReactNode;
		children?: ReactNode;
		header?: ReactNode;
		formId?: string;
		className?: string;
	};

export type StepperFormValues<Steps extends Step[]> = z.TypeOf<
	Steps[number]["schema"] extends z.ZodSchema
		? Steps[number]["schema"]
		: Steps[number]["schema"] extends (
					values: Record<string, unknown>,
				) => z.ZodSchema
			? ReturnType<Steps[number]["schema"]>
			: never
>;

export type ExtractSteps<T> =
	T extends StepperFormProps<infer Steps> ? Steps : never;

export function StepperForm<const Steps extends Step[]>({
	children,
	className,
	...props
}: StepperFormProps<Steps>) {
	const Stepper = props.Stepper;
	return (
		<Stepper.Provider
			className={className}
			variant={props.singlePage ? "circle" : "vertical"}
			initialStep={props.initialStep}
		>
			<Content<Steps> {...props} extraChildren={children} />
		</Stepper.Provider>
	);
}

// Cross-fade between steps. The step container clips overflow so it can animate
// its height between differently-sized steps; a horizontal slide would be cut
// off by that same clip, so the transition is opacity-only and the height
// reveal carries the motion.
const slideVariants = {
	enter: { opacity: 0 },
	center: { opacity: 1 },
	exit: { opacity: 0 },
};

// Animates its own height to match the measured height of the current child so
// stepping between steps of different sizes glides instead of snapping. The
// inner content height is measured continuously; the wrapper animates to it and
// clips overflow during the transition.
// Resolves a step's title/description against the live form values so headings
// can adapt to earlier choices. Subscribes via useWatch so it re-renders.
function StepHeading({ step }: { step: Step }) {
	const values = useWatch() as Record<string, unknown>;
	const title = step.titleFor ? step.titleFor(values) : step.title;
	const description =
		typeof step.description === "function"
			? step.description(values)
			: step.description;
	return (
		<>
			<div className="flex items-center justify-between">
				<h2 className="text-xl font-semibold">{title}</h2>
			</div>
			{description ? (
				<p className="mt-1.5 text-sm text-muted-foreground">
					{description}
				</p>
			) : null}
		</>
	);
}

function AnimatedHeight({ children }: { children: ReactNode }) {
	const innerRef = useRef<HTMLDivElement>(null);
	const [height, setHeight] = useState<number | "auto">("auto");

	useLayoutEffect(() => {
		const el = innerRef.current;
		if (!el) return;
		const measure = () => setHeight(el.offsetHeight);
		measure();
		const observer = new ResizeObserver(measure);
		observer.observe(el);
		return () => observer.disconnect();
	}, []);

	return (
		<motion.div
			animate={{ height }}
			transition={{ duration: 0.25, ease: [0.4, 0, 0.2, 1] }}
			style={{ overflow: "hidden" }}
		>
			<div ref={innerRef}>{children}</div>
		</motion.div>
	);
}

function getNextVisibleStepId(
	steps: Step[],
	currentId: string,
	values: Record<string, unknown>,
): string | null {
	const currentIndex = steps.findIndex((s) => s.id === currentId);
	for (let i = currentIndex + 1; i < steps.length; i++) {
		const step = steps[i];
		if (!step.isVisible || step.isVisible(values)) return step.id;
	}
	return null;
}

function getPrevVisibleStepId(
	steps: Step[],
	currentId: string,
	values: Record<string, unknown>,
): string | null {
	const currentIndex = steps.findIndex((s) => s.id === currentId);
	for (let i = currentIndex - 1; i >= 0; i--) {
		const step = steps[i];
		if (!step.isVisible || step.isVisible(values)) return step.id;
	}
	return null;
}

function Content<const Steps extends Step[]>({
	defaultValues,
	Stepper,
	useStepper,
	steps: allSteps,
	content,
	showAllSteps,
	singlePage,
	onSubmit,
	onPartialSubmit,
	initialStep,
	controls,
	formId,
	header,
	extraChildren,
	...formProps
}: StepperFormProps<Steps> & { extraChildren?: ReactNode }) {
	const stepper = useStepper({ initialStep });

	const resolveSchema = (step: Step, values: Record<string, unknown>) => {
		if (typeof step.schema === "function") return step.schema(values);
		return step.schema;
	};

	const form = useForm<z.infer<JoinStepSchemas<Steps>>>({
		defaultValues,
		mode: "onChange",
		reValidateMode: "onChange",
		shouldFocusError: false,
		...formProps,
		...(stepper.current.schema
			? {
					resolver: (values, context, options) => {
						const accumulated = {
							...(ref.current ?? {}),
							...values,
						} as Record<string, unknown>;
						const schema = resolveSchema(
							stepper.current as unknown as Step,
							accumulated,
						);
						// @ts-expect-error - we know this is correct based on the definition of Step
						return zodResolver(schema)(values, context, options);
					},
				}
			: {}),
	});

	// Step visibility can depend on form values. Keep the visibility provider
	// subscribed to the live form state so sequential changes (including
	// dirty-to-dirty changes) update progress and the visible step sequence.
	const liveValues = (useWatch({ control: form.control }) ?? {}) as Record<
		string,
		unknown
	>;

	const ref = useRef<z.infer<JoinStepSchemas<Steps>> | null>({});
	const formRef = useRef<HTMLFormElement>(null);

	const getValues = () =>
		({ ...ref.current, ...liveValues }) as Record<string, unknown>;

	const isLastVisible = (currentId: string) =>
		getNextVisibleStepId(allSteps as Step[], currentId, getValues()) ===
		null;

	const handleSubmit = async (values: z.infer<JoinStepSchemas<Steps>>) => {
		ref.current = { ...ref.current, ...values };

		if (formId) {
			posthog.capture(formId, {
				step: stepper.current.id,
				group: stepper.current.group,
				values: ref.current,
			});
		}
		if (isLastVisible(stepper.current.id)) {
			return onSubmit?.({ values: ref.current, form, stepper });
		}
		await onPartialSubmit?.({ values: ref.current, form, stepper });
		// `liveValues` is the previous render's useWatch snapshot, so it still
		// holds the pre-submit value for anything just changed. Let the
		// accumulated values win here or a step gated on a field submitted by
		// this step resolves its visibility against the stale choice.
		const nextId = getNextVisibleStepId(
			allSteps as Step[],
			stepper.current.id,
			{ ...getValues(), ...ref.current },
		);
		if (nextId) stepper.goTo(nextId as Parameters<typeof stepper.goTo>[0]);
		form.reset(undefined, {
			keepErrors: false,
			keepValues: true,
			keepDirty: false,
		});
		form.clearErrors();
	};

	const visibleSteps = (allSteps as Step[]).filter(
		(s) => !s.isVisible || s.isVisible(getValues()),
	);
	const visibilityContext: StepVisibilityContextType = {
		isStepVisible: (stepId) => {
			const step = (allSteps as Step[]).find((s) => s.id === stepId);
			return !step?.isVisible || step.isVisible(getValues());
		},
		visibleStepIndex: (stepId) =>
			visibleSteps.findIndex((s) => s.id === stepId),
		visibleStepCount: visibleSteps.length,
	};

	if (singlePage) {
		const step = stepper.current;
		const hasPrev =
			getPrevVisibleStepId(allSteps as Step[], step.id, getValues()) !==
			null;
		return (
			<StepVisibilityContext.Provider value={visibilityContext}>
				<StepperFormContext.Provider
					value={{
						submitForm: () => formRef.current?.requestSubmit(),
					}}
				>
					<FormProvider {...form}>
						{header}
						<form
							ref={formRef}
							onSubmit={(event) => {
								event.stopPropagation();
								return form.handleSubmit(handleSubmit)(event);
							}}
							className="space-y-6"
						>
							<AnimatedHeight>
								<AnimatePresence mode="wait">
									<motion.div
										key={step.id}
										variants={slideVariants}
										initial="enter"
										animate="center"
										exit="exit"
										transition={{
											duration: 0.25,
											ease: [0.4, 0, 0.2, 1],
										}}
									>
										<StepHeading step={step as Step} />
										<div className="mt-6">
											<StepPanel<Steps>
												Stepper={Stepper}
												stepper={stepper}
												allSteps={allSteps as Step[]}
												valuesRef={
													ref as MutableRefObject<Record<
														string,
														unknown
													> | null>
												}
												step={step}
												content={content}
												controls={controls}
												showNext={step.showNext ?? true}
												showPrevious={
													(step.showPrevious ??
														true) &&
													hasPrev
												}
											/>
										</div>
										{extraChildren}
									</motion.div>
								</AnimatePresence>
							</AnimatedHeight>
						</form>
					</FormProvider>
				</StepperFormContext.Provider>
			</StepVisibilityContext.Provider>
		);
	}

	return (
		<StepVisibilityContext.Provider value={visibilityContext}>
			<Stepper.Navigation>
				<StepperFormContext.Provider
					value={{
						submitForm: () => formRef.current?.requestSubmit(),
					}}
				>
					<FormProvider {...form}>
						<form
							ref={formRef}
							onSubmit={(event) => {
								event.stopPropagation();
								return form.handleSubmit(handleSubmit)(event);
							}}
						>
							{stepper.all.map((step, index, steps) => (
								<Stepper.Step
									key={step.id}
									className="min-w-0 w-full"
									of={step.id}
								>
									<Stepper.Title>{step.title}</Stepper.Title>

									{showAllSteps ? (
										<StepPanel<Steps>
											key={step.id}
											Stepper={Stepper}
											stepper={stepper}
											allSteps={allSteps as Step[]}
											valuesRef={
												ref as MutableRefObject<Record<
													string,
													unknown
												> | null>
											}
											step={step}
											content={content}
											showNext={false}
											showPrevious={false}
											showControls={
												steps.length - 1 === index
											}
											controls={controls}
										/>
									) : (
										stepper.when(step.id, (step) => {
											return (
												<StepPanel<Steps>
													Stepper={Stepper}
													stepper={stepper}
													allSteps={
														allSteps as Step[]
													}
													valuesRef={
														ref as MutableRefObject<Record<
															string,
															unknown
														> | null>
													}
													step={step}
													content={content}
													controls={controls}
													showNext={
														step.showNext ?? true
													}
													showPrevious={
														step.showPrevious ??
														true
													}
												/>
											);
										})
									)}
								</Stepper.Step>
							))}
						</form>
					</FormProvider>
				</StepperFormContext.Provider>
			</Stepper.Navigation>
		</StepVisibilityContext.Provider>
	);
}

function StepPanel<const Steps extends Step[]>({
	Stepper,
	stepper,
	allSteps,
	valuesRef,
	step,
	content,
	showNext = true,
	showPrevious = true,
	showControls = true,
	controls,
}: Pick<StepperFormProps<Steps>, "Stepper" | "content"> & {
	stepper: Stepperize.Stepper<Steps>;
	allSteps: Step[];
	valuesRef: MutableRefObject<Record<string, unknown> | null>;
	step: Steps[number];
	showControls?: boolean;
	showNext?: boolean;
	showPrevious?: boolean;
	controls?: ReactNode;
}) {
	const form = useFormContext();
	const liveValues = useWatch({ control: form.control });
	const mergedValues = {
		...(valuesRef.current ?? {}),
		...liveValues,
	} as Record<string, unknown>;
	const stepSchema =
		typeof step.schema === "function"
			? step.schema(mergedValues)
			: step.schema;
	const isStepValid = stepSchema
		? stepSchema.safeParse(mergedValues).success
		: true;

	const [confirmNode, setConfirmNode] = useState<ReactNode | null>(null);
	const hiddenSubmitRef = useRef<HTMLButtonElement>(null);
	const stepConfirm = (step as Step).confirm;

	const onNextClick = async (e: React.MouseEvent<HTMLButtonElement>) => {
		if (!stepConfirm) return;
		e.preventDefault();
		const valid = await form.trigger();
		if (!valid) return;
		const result = await stepConfirm(mergedValues);
		if (result == null) {
			hiddenSubmitRef.current?.click();
		} else {
			setConfirmNode(result);
		}
	};

	const goToPrev = () => {
		const allLive = form.getValues() as Record<string, unknown>;
		const dirtyFields = form.formState.dirtyFields as Record<
			string,
			unknown
		>;
		const live = Object.fromEntries(
			Object.entries(allLive).filter(([k]) => k in dirtyFields),
		);
		const mergedValues = {
			...(valuesRef.current ?? {}),
			...live,
		} as Record<string, unknown>;
		form.reset(undefined, { keepErrors: false, keepValues: true });
		const prevId = getPrevVisibleStepId(allSteps, step.id, mergedValues);
		if (prevId) {
			stepper.goTo(prevId as Parameters<typeof stepper.goTo>[0]);
		}
	};

	return (
		<Stepper.Panel className="space-y-6">
			{stepper.match(step.id, content)}
			{confirmNode ? (
				<p className="text-sm text-muted-foreground">
					<Icon
						icon={faTriangleExclamation}
						className="text-destructive mr-1.5"
					/>
					{confirmNode}
				</p>
			) : null}
			{showControls ? (
				<Stepper.Controls
					className={cn(
						"items-center",
						stepper.isLast && !showNext && "justify-start",
					)}
				>
					<button
						type="submit"
						ref={hiddenSubmitRef}
						className="hidden"
						tabIndex={-1}
						aria-hidden
					/>
					{controls}
					{showPrevious ? (
						<Button
							type="button"
							variant="outline"
							size={step.previous ? undefined : "icon"}
							onClick={goToPrev}
							startIcon={
								step.previous ? (
									<Icon icon={faArrowLeft} />
								) : undefined
							}
						>
							{step.previous ? (
								step.previous
							) : (
								<Icon icon={faArrowLeft} />
							)}
						</Button>
					) : null}
					{showNext && confirmNode ? (
						<>
							<Button
								type="button"
								variant="secondary"
								onClick={() => setConfirmNode(null)}
								disabled={form.formState.isSubmitting}
							>
								Cancel
							</Button>
							<Button
								type="submit"
								variant="destructive"
								isLoading={form.formState.isSubmitting}
							>
								{step.next ? `Confirm ${step.next}` : "Confirm"}
							</Button>
						</>
					) : showNext ? (
						<Button
							type={stepConfirm ? "button" : "submit"}
							size={step.next ? undefined : "icon"}
							onClick={stepConfirm ? onNextClick : undefined}
							disabled={!isStepValid}
							isLoading={form.formState.isSubmitting}
						>
							{step.next ? (
								step.next
							) : (
								<Icon icon={faArrowRight} />
							)}
						</Button>
					) : null}
				</Stepper.Controls>
			) : null}
		</Stepper.Panel>
	);
}
