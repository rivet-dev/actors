import { faVercel, Icon } from "@rivet-gg/icons";
import type { Rivet } from "@rivetkit/engine-api-full";
import {
	useMutation,
	usePrefetchInfiniteQuery,
	useSuspenseInfiniteQuery,
} from "@tanstack/react-query";
import { useWatch } from "react-hook-form";
import * as ConnectVercelForm from "@/app/forms/connect-vercel-form";
import { type DialogContentProps, Frame } from "@/components";
import { useEngineCompatDataProvider } from "@/components/actors";
import { successfulBackendSetupEffect } from "@/lib/effects";
import { queryClient } from "@/queries/global";
import { StepperForm } from "../forms/stepper-form";
import {
	buildServerlessConfig,
	ConfigurationAccordion,
} from "./connect-manual-serverless-frame";

const { stepper } = ConnectVercelForm;

interface CreateProjectFrameContentProps extends DialogContentProps {}

export default function CreateProjectFrameContent({
	onClose,
}: CreateProjectFrameContentProps) {
	usePrefetchInfiniteQuery({
		...useEngineCompatDataProvider().datacentersQueryOptions(),
		pages: Infinity,
	});

	const { data: datacenters } = useSuspenseInfiniteQuery(
		useEngineCompatDataProvider().datacentersQueryOptions(),
	);

	return (
		<>
			<Frame.Header>
				<Frame.Title className="gap-2 flex items-center">
					<div>
						Add <Icon icon={faVercel} className="ml-0.5" />
						Vercel
					</div>
				</Frame.Title>
			</Frame.Header>
			<Frame.Content>
				<FormStepper onClose={onClose} datacenters={datacenters} />
			</Frame.Content>
		</>
	);
}

function FormStepper({
	datacenters,
	onClose,
}: {
	onClose?: () => void;
	datacenters: Rivet.Datacenter[];
}) {
	const provider = useEngineCompatDataProvider();

	const { mutateAsync } = useMutation({
		...provider.upsertRunnerConfigMutationOptions(),
		onSuccess: async () => {
			successfulBackendSetupEffect();
			await queryClient.invalidateQueries(
				provider.runnerConfigsQueryOptions(),
			);
			onClose?.();
		},
	});
	return (
		<StepperForm
			{...stepper}
			content={{
				"api-route": () => <StepApiRoute />,
				frontend: () => <StepFrontend />,
				variables: () => (
					<>
						<p>
							Set these variables in Settings &gt; Environment
							Variables in the Vercel dashboard.
						</p>
						<ConnectVercelForm.EnvVariables />
					</>
				),
				deploy: () => <StepDeploy />,
			}}
			onSubmit={async ({ values }) => {
				const payload = await buildServerlessConfig(
					provider,
					{
						...values,
						requestLifespan:
							ConnectVercelForm.VERCEL_REQUEST_LIFESPAN,
					},
					{ provider: "vercel" },
				);

				await mutateAsync({
					name: values.runnerName,
					config: payload,
				});
			}}
			defaultValues={{
				plan: "hobby",
				runnerName: "default",
				drainGracePeriod: ConnectVercelForm.VERCEL_DRAIN_GRACE_PERIOD,
				headers: [],
				datacenters: Object.fromEntries(
					datacenters.map((dc) => [dc.name, true]),
				),
			}}
		/>
	);
}

function StepApiRoute() {
	const plan = useWatch({ name: "plan" });
	return <ConnectVercelForm.IntegrationCode plan={plan || "hobby"} />;
}

function StepFrontend() {
	return <ConnectVercelForm.FrontendIntegrationCode />;
}

function StepDeploy() {
	return (
		<>
			<p>
				Deploy your code to Vercel and paste your deployment's endpoint:
			</p>
			<div className="mt-2">
				<ConnectVercelForm.Endpoint />
				<ConfigurationAccordion
					requestLifespan={false}
					prefixFields={<ConnectVercelForm.Plan />}
				/>
				<p className="text-muted-foreground text-sm">
					Need help deploying? See{" "}
					<a
						href="https://vercel.com/docs/deployments"
						target="_blank"
						rel="noreferrer"
						className="underline"
					>
						Vercel's deployment documentation
					</a>
					.
				</p>
			</div>

			<ConnectVercelForm.ConnectionCheck provider="vercel" />
		</>
	);
}
