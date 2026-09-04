import { useDialog as baseUseDialog, createDialogHook } from "@/components";

export const useDialog = {
	...baseUseDialog,
	AddComponent: createDialogHook(
		() => import("@/app/dialogs/add-component-frame"),
	),
	CreateNamespace: createDialogHook(
		() => import("@/app/dialogs/create-namespace-frame"),
	),
	CreateProject: createDialogHook(
		() => import("@/app/dialogs/create-project-frame"),
	),
	ConnectRivet: createDialogHook(
		() => import("@/app/dialogs/connect-rivet-frame"),
	),
	ConnectVercel: createDialogHook(
		() => import("@/app/dialogs/connect-vercel-frame"),
	),
	ConnectQuickVercel: createDialogHook(
		() => import("@/app/dialogs/connect-quick-vercel-frame"),
	),
	ConnectRailway: createDialogHook(
		() => import("@/app/dialogs/connect-railway-frame"),
	),
	ConnectQuickRailway: createDialogHook(
		() => import("@/app/dialogs/connect-quick-railway-frame"),
	),
	ConnectManual: createDialogHook(
		() => import("@/app/dialogs/connect-manual-frame"),
	),
	ConnectAws: createDialogHook(
		() => import("@/app/dialogs/connect-aws-frame"),
	),
	ConnectGcp: createDialogHook(
		() => import("@/app/dialogs/connect-gcp-frame"),
	),
	ConnectHetzner: createDialogHook(
		() => import("@/app/dialogs/connect-hetzner-frame"),
	),
	EditProviderConfig: createDialogHook(
		() => import("@/app/dialogs/edit-runner-config"),
	),
	DeleteConfig: createDialogHook(
		() => import("@/app/dialogs/confirm-delete-config-frame"),
	),
	DeleteNamespace: createDialogHook(
		() => import("@/app/dialogs/confirm-delete-namespace-frame"),
	),
	DeleteProject: createDialogHook(
		() => import("@/app/dialogs/confirm-delete-project-frame"),
	),
	Billing: createDialogHook(() => import("@/app/dialogs/billing-frame")),
	ProvideEngineCredentials: createDialogHook(
		() => import("@/app/dialogs/provide-engine-credentials-frame"),
	),
	Tokens: createDialogHook(() => import("@/app/dialogs/tokens-frame")),
	ApiTokens: createDialogHook(() => import("@/app/dialogs/api-tokens-frame")),
	CreateApiToken: createDialogHook(
		() => import("@/app/dialogs/create-api-token-frame"),
	),
	CreateOrganization: createDialogHook(
		() => import("@/app/dialogs/create-organization-frame"),
	),
	UpsertDeployment: createDialogHook(
		() => import("@/app/dialogs/upsert-deployment-frame"),
	),
	OrgMembers: createDialogHook(
		() => import("@/app/dialogs/org-members-frame"),
	),
};
