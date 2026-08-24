import type { Rivet } from "@rivetkit/engine-api-full";
import { deployOptions, type Provider } from "@rivetkit/shared-data";
import {
	useMutation,
	usePrefetchInfiniteQuery,
	useSuspenseInfiniteQuery,
} from "@tanstack/react-query";
import type { ComponentProps, ReactNode } from "react";
import { useWatch } from "react-hook-form";
import z from "zod";
import * as ConnectServerlessForm from "@/app/forms/connect-manual-serverless-form";
import {
	Accordion,
	AccordionContent,
	AccordionItem,
	AccordionTrigger,
	type DialogContentProps,
} from "@/components";
import { useEngineCompatDataProvider } from "@/components/actors";
import { defineStepper } from "@/components/ui/stepper";
import { successfulBackendSetupEffect } from "@/lib/effects";
import { queryClient } from "@/queries/global";
import { EnvVariables } from "../env-variables";
import { StepperForm } from "../forms/stepper-form";
import { useEndpoint } from "./connect-manual-serverful-frame";

const stepper = defineStepper(
	{
		id: "step-1",
		title: "Configure",
		assist: false,
		schema: ConnectServerlessForm.configurationSchema,
	},
	{
		id: "step-2",
		title: "Deploy",
		assist: false,
		schema: z.object({}),
	},
	{
		id: "step-3",
		title: "Confirm Connection",
		assist: false,
		schema: ConnectServerlessForm.deploymentSchema,
		next: "Add",
	},
);

interface ConnectManualServerlessFrameContentProps extends DialogContentProps {
	provider: Provider;
}

export default function ConnectManualServerlessFrameContent({
	onClose,
	provider,
}: ConnectManualServerlessFrameContentProps) {
	usePrefetchInfiniteQuery({
		...useEngineCompatDataProvider().datacentersQueryOptions(),
		pages: Infinity,
	});

	const { data: datacenters } = useSuspenseInfiniteQuery(
		useEngineCompatDataProvider().datacentersQueryOptions(),
	);

	return (
		<FormStepper
			onClose={onClose}
			datacenters={datacenters}
			provider={provider}
		/>
	);
}

function FormStepper({
	onClose,
	datacenters,
	provider,
}: {
	onClose?: () => void;
	datacenters: Rivet.Datacenter[];
	provider: Provider;
}) {
	const dataProvider = useEngineCompatDataProvider();

	const { mutateAsync } = useMutation({
		...dataProvider.upsertRunnerConfigMutationOptions(),
		onSuccess: async () => {
			successfulBackendSetupEffect();

			await queryClient.invalidateQueries(
				dataProvider.runnerConfigsQueryOptions(),
			);
			onClose?.();
		},
	});
	return (
		<StepperForm
			{...stepper}
			onSubmit={async ({ values }) => {
				const payload = await buildServerlessConfig(
					dataProvider,
					values,
					{
						provider,
					},
				);

				await mutateAsync({
					name: values.runnerName,
					config: payload,
				});
			}}
			defaultValues={{
				runnerName: "default",
				headers: [],
				requestLifespan: 900,
				drainGracePeriod: 0,
				datacenters: Object.fromEntries(
					datacenters.map((dc) => [dc.name, true]),
				),
			}}
			content={{
				"step-1": () => <Step1 provider={provider} />,
				"step-2": () => <Step2 provider={provider} />,
				"step-3": () => <Step3 provider={provider} />,
			}}
		/>
	);
}

export const buildServerlessConfig = async (
	dataProvider: ReturnType<typeof useEngineCompatDataProvider>,
	values: z.infer<
		typeof ConnectServerlessForm.configurationSchema &
			typeof ConnectServerlessForm.deploymentSchema
	>,
	{ provider }: { provider?: string } = {},
): Promise<Record<string, Rivet.RunnerConfig>> => {
	const status = await queryClient.ensureQueryData(
		dataProvider.runnerHealthCheckQueryOptions({
			runnerUrl: ConnectServerlessForm.endpointSchema.parse(
				values.endpoint,
			),
			headers: Object.fromEntries(values.headers),
		}),
	);

	const endpoint = status.url || values.endpoint;

	const runnerConfig = await queryClient.fetchQuery(
		dataProvider.runnerConfigQueryOptions({
			name: values.runnerName,
			safe: true,
		}),
	);
	const existing: Record<string, Rivet.RunnerConfig> =
		runnerConfig?.datacenters || {};

	const selectedDatacenters = Object.entries(values.datacenters)
		.filter(([, selected]) => selected)
		.map(([id]) => id);

	const headers = Object.fromEntries(
		values.headers.map(([key, value]) => [key, value]),
	);

	// maxConcurrentActors is not set during onboarding; the backend default applies.
	const payload = {
		...existing,
		...Object.fromEntries(
			selectedDatacenters.map((dc) => {
				const isNew = (existing[dc] as any)?.protocolVersion != null;
				const serverless: Rivet.RunnerConfigServerless = isNew
					? {
							url: endpoint,
							requestLifespan: values.requestLifespan,
							headers,
							maxRunners: 0,
							slotsPerRunner: 1,
							drainGracePeriod: values.drainGracePeriod,
						}
					: {
							url: endpoint,
							requestLifespan: values.requestLifespan,
							headers,
							maxRunners: values.maxRunners ?? 100_000,
							slotsPerRunner: values.slotsPerRunner ?? 1,
							runnersMargin: values.runnerMargin ?? 0,
							minRunners: values.minRunners ?? 0,
						};
				const resolvedProvider = provider || "custom";
				const isCustom =
					resolvedProvider === "custom" ||
					resolvedProvider === "custom-platform";
				const customName = isCustom
					? values.customName?.trim() || undefined
					: undefined;
				const customIcon = isCustom
					? values.customIcon || undefined
					: undefined;
				const config = {
					serverless,
					metadata: {
						provider: resolvedProvider,
						...(customName ? { customName } : {}),
						...(customIcon ? { customIcon } : {}),
					},
				};
				return [dc, config];
			}),
		),
	};

	return payload;
};

function Step1({ provider }: { provider: Provider }) {
	const isCustom = provider === "custom" || provider === "custom-platform";
	return (
		<div className="space-y-4">
			{isCustom ? <ConnectServerlessForm.CustomBranding /> : null}
			<Configuration />
		</div>
	);
}

function Step2({ provider }: { provider: Provider }) {
	const providerOptions = deployOptions.find(
		(option) => option.name === provider,
	);
	return (
		<>
			<p>
				<a
					href={`https://www.rivet.dev${providerOptions?.href || "/docs/getting-started"}`}
					className="underline"
					target="_blank"
					rel="noopener"
				>
					Follow the integration guide here
				</a>
				, and make sure to set the following environment variables:
			</p>
			<EnvVariables
				endpoint={useEndpoint()}
				runnerName={useWatch({ name: "runnerName" })}
			/>
		</>
	);
}

function Step3({ provider }: { provider: Provider }) {
	return (
		<>
			<ConnectServerlessForm.Endpoint placeholder="https://your-serverless-endpoint.com/api/rivet" />
			<ConnectServerlessForm.ConnectionCheck provider={provider} />
		</>
	);
}

export function Configuration({
	runnerName = true,
	datacenters = true,
	headers = true,
	requestLifespan = true,
	drainGracePeriod = true,
}: {
	runnerName?: boolean;
	datacenters?: boolean;
	headers?: boolean;
	requestLifespan?: boolean;
	drainGracePeriod?: boolean;
}) {
	return (
		<>
			{runnerName && <ConnectServerlessForm.RunnerName />}
			{datacenters && <ConnectServerlessForm.Datacenters />}
			{headers && <ConnectServerlessForm.Headers />}
			{requestLifespan && <ConnectServerlessForm.RequestLifespan />}
			{drainGracePeriod && <ConnectServerlessForm.DrainGracePeriod />}
		</>
	);
}

export function ConfigurationAccordion({
	prefixFields,
	...props
}: ComponentProps<typeof Configuration> & { prefixFields?: ReactNode }) {
	return (
		<Accordion type="single" collapsible>
			<AccordionItem value="item-1">
				<AccordionTrigger className="text-sm">
					Advanced
				</AccordionTrigger>
				<AccordionContent className="space-y-4 px-1 pt-2">
					{prefixFields}
					<Configuration {...props} />
				</AccordionContent>
			</AccordionItem>
		</Accordion>
	);
}
