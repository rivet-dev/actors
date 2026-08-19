import z from "zod";
import * as ConnectVercelForm from "@/app/forms/connect-vercel-form";
import { defineStepper } from "@/components/ui/stepper";
import { deploymentSchema } from "./connect-manual-serverless-form";

export const VERCEL_REQUEST_LIFESPAN =
	ConnectVercelForm.VERCEL_REQUEST_LIFESPAN;
export const VERCEL_DRAIN_GRACE_PERIOD =
	ConnectVercelForm.VERCEL_DRAIN_GRACE_PERIOD;

export const stepper = defineStepper(
	{
		id: "initial-info",
		title: "Configure",
		assist: false,
		next: "Next",
		schema: z.object({}),
	},
	{
		id: "deploy",
		title: "Configure Vercel endpoint",
		assist: true,
		next: "Done",
		schema: z.object({
			...ConnectVercelForm.configurationSchema.shape,
			...deploymentSchema.shape,
			plan: z.string().min(1, "Please select a Vercel plan"),
		}),
	},
);

export const RunnerName = ConnectVercelForm.RunnerName;

export const Datacenters = ConnectVercelForm.Datacenters;

export const MinRunners = ConnectVercelForm.MinRunners;

export const MaxRunners = ConnectVercelForm.MaxRunners;

export const SlotsPerRunner = ConnectVercelForm.SlotsPerRunner;

export const RunnerMargin = ConnectVercelForm.RunnerMargin;

export const Headers = ConnectVercelForm.Headers;

export const Endpoint = ConnectVercelForm.Endpoint;

export const ConnectionCheck = ConnectVercelForm.ConnectionCheck;

export const Plan = ConnectVercelForm.Plan;

export const EnvVariables = ConnectVercelForm.EnvVariables;
