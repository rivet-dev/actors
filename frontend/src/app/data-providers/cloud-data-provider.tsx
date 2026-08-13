import { type Rivet, RivetClient } from "@rivet-gg/cloud";
import { fetcher } from "@rivetkit/engine-api-full/core";
import {
	infiniteQueryOptions,
	mutationOptions,
	type QueryKey,
	queryOptions,
	type UseQueryOptions,
} from "@tanstack/react-query";
import { authClient } from "@/lib/auth";
import { cloudEnv } from "@/lib/env";
import { queryClient } from "@/queries/global";
import { RECORDS_PER_PAGE } from "./default-data-provider";
import {
	type CreateNamespace,
	createClient as createEngineClient,
	createNamespaceContext as createEngineNamespaceContext,
	type Namespace,
} from "./engine-data-provider";
import { no404Retry } from "./utilities";

type CloudFetchFunction = NonNullable<RivetClient.Options["fetcher"]>;
type CloudFetchResult = Awaited<ReturnType<CloudFetchFunction>>;
type CloudRawResponse = CloudFetchResult["rawResponse"];

async function resolveCloudHeaders(
	headers: Parameters<CloudFetchFunction>[0]["headers"],
): Promise<Record<string, string | undefined>> {
	const resolvedHeaders: Record<string, string | undefined> = {};
	for (const [key, value] of Object.entries(headers ?? {})) {
		if (key.toLowerCase().startsWith("x-fern-") || value == null) {
			continue;
		}
		const resolved =
			typeof value === "function" ? await value() : await value;
		if (resolved != null) {
			resolvedHeaders[key] = resolved;
		}
	}
	return resolvedHeaders;
}

function cloudRawResponse(
	url: string,
	result: Awaited<ReturnType<typeof fetcher>>,
): CloudRawResponse {
	const status = result.ok
		? 200
		: result.error.reason === "status-code" ||
				result.error.reason === "non-json"
			? result.error.statusCode
			: result.error.reason === "timeout"
				? 499
				: 0;
	return {
		headers: new Headers(result.ok ? result.headers : undefined),
		redirected: false,
		status,
		statusText: result.ok ? "OK" : "",
		type: result.ok ? "basic" : "error",
		url,
	};
}

const cloudFetcher: CloudFetchFunction = async <R = unknown>(
	args: Parameters<CloudFetchFunction>[0],
) => {
	const result = await fetcher<R>({
		...args,
		headers: await resolveCloudHeaders(args.headers),
		queryParameters: args.queryParameters as Parameters<
			typeof fetcher
		>[0]["queryParameters"],
		responseType:
			args.responseType === "binary-response"
				? "arrayBuffer"
				: args.responseType,
		maxRetries: 1,
		withCredentials: true,
	});
	return {
		...result,
		rawResponse: cloudRawResponse(args.url, result),
	};
};

function createClient() {
	return new RivetClient({
		baseUrl: () => cloudEnv().VITE_APP_CLOUD_API_URL,
		environment: "",
		token: async () => "",
		fetcher: cloudFetcher,
	});
}

export const createGlobalContext = () => {
	const client = createClient();
	return {
		client,
		organizationQueryOptions(opts: { org: string }) {
			return queryOptions({
				queryKey: ["organization", opts.org],
				queryFn: async () => {
					const result =
						await authClient.organization.getFullOrganization({
							query: { organizationSlug: opts.org },
						});
					return result.data;
				},
			});
		},
		billingCustomerPortalSessionQueryOptions() {
			return queryOptions({
				staleTime: 5 * 60 * 1000, // 5 minutes
				gcTime: 5 * 60 * 1000, // 5 minutes
				queryKey: ["billing-customer-portal-session"],
				queryFn: async () => {
					const session =
						await client.billing.createCustomerPortalSession();
					if (!session.url) {
						throw new Error(
							"No URL returned for customer portal session",
						);
					}
					return session.url;
				},
			});
		},
		billingDetailsQueryOptions({
			organization,
			project,
		}: {
			organization: string;
			project: string;
		}) {
			return queryOptions({
				queryKey: [{ organization, project }, "billing-details"],
				queryFn: async () => {
					const response = await client.billing.details(project, {
						org: organization,
					});
					return response;
				},
			});
		},
		billingUsageQueryOptions({
			organization,
			project,
		}: {
			organization: string;
			project: string;
		}) {
			return queryOptions({
				queryKey: [{ organization, project }, "billing-usage"],
				// The usage query is a slow (~10s) backend scan whose numbers
				// barely move minute to minute, so serve it from cache rather
				// than refetch on every mount.
				staleTime: 5 * 60 * 1000, // 5 minutes
				gcTime: 5 * 60 * 1000, // 5 minutes
				queryFn: async () => {
					const response = await client.billing.usage(project, {
						org: organization,
					});
					return response;
				},
			});
		},
		managedPoolsQueryOptions(opts: {
			organization: string;
			project: string;
			namespace: string;
		}) {
			return queryOptions({
				queryKey: [opts, "managed-pools"],
				queryFn: async () => {
					const response = await client.managedPools.list(
						opts.project,
						opts.namespace,
						{
							org: opts.organization,
						},
					);
					// Hide tearing-down pools from switchers and resolution,
					// matching `rivet pool list`.
					return response.managedPools.filter(
						(pool) => pool.status !== "destroying",
					);
				},
				...no404Retry(),
			});
		},
		managedPoolQueryOptions(opts: {
			organization: string;
			project: string;
			namespace: string;
			pool: string;
			safe?: boolean;
		}) {
			return queryOptions({
				queryKey: [
					{
						organization: opts.organization,
						project: opts.project,
						namespace: opts.namespace,
						pool: opts.pool,
					},
					"managed-pool",
				],
				queryFn: async () => {
					try {
						const response = await client.managedPools.get(
							opts.project,
							opts.namespace,
							opts.pool,
							{
								org: opts.organization,
							},
						);

						return response.managedPool;
					} catch (err) {
						if (opts.safe) {
							return null;
						}
						throw err;
					}
				},
				...no404Retry(),
			});
		},

		imageRepositoriesQueryOptions(opts: {
			organization: string;
			project: string;
		}) {
			return infiniteQueryOptions({
				queryKey: [opts, "image-repositories"],
				queryFn: async ({ pageParam }) => {
					return await client.docker.listRepositories(opts.project, {
						limit: 10,
						cursor: pageParam ?? undefined,
						org: opts.organization,
					});
				},
				getNextPageParam: (lastPage) => {
					return lastPage.pagination.cursor;
				},
				initialPageParam: undefined as string | undefined,
				select: (data) =>
					data.pages.flatMap((page) => page.repositories),
			});
		},

		tagsQueryOptions(opts: {
			repository: string;
			project: string;
			organization: string;
		}) {
			return infiniteQueryOptions({
				queryKey: [opts, "tags"],
				queryFn: async ({ pageParam }) => {
					return await client.docker.listTags(
						opts.project,
						opts.repository,
						{
							limit: 10,
							cursor: pageParam ?? undefined,
							org: opts.organization,
						},
					);
				},
				getNextPageParam: (lastPage) => {
					return lastPage.pagination.cursor;
				},
				initialPageParam: undefined as string | undefined,
				select: (data) => data.pages.flatMap((page) => page.tags),
			});
		},

		imagesQueryOptions(opts: {
			organization: string;
			project: string;
			limit?: number;
		}) {
			return infiniteQueryOptions({
				queryKey: [opts, "images"],
				queryFn: async ({ pageParam }) => {
					return await client.docker.listImages(opts.project, {
						limit: opts.limit ?? 10,
						cursor: pageParam ?? undefined,
						org: opts.organization,
					});
				},
				getNextPageParam: (lastPage) => {
					return lastPage.pagination.cursor;
				},
				initialPageParam: undefined as string | undefined,
				select: (data) => data.pages.flatMap((page) => page.images),
			});
		},
	};
};

export const createOrganizationContext = ({
	client,
	organization,
	...parent
}: {
	organization: string;
} & ReturnType<typeof createGlobalContext>) => {
	const orgProjectNamespacesQueryOptions = (opts: {
		organization: string;
		project: string;
	}) =>
		infiniteQueryOptions({
			queryKey: [opts, "namespaces"],
			initialPageParam: undefined as string | undefined,
			queryFn: async ({ pageParam }) => {
				const data = await client.namespaces.list(opts.project, {
					org: opts.organization,
					limit: 100,
					cursor: pageParam ?? undefined,
				});
				return {
					pagination: data.pagination,
					namespaces: data.namespaces.map((ns) => ({
						id: ns.id,
						name: ns.name,
						displayName: ns.displayName,
						createdAt: ns.createdAt,
					})),
				};
			},
			getNextPageParam: (lastPage) => {
				if (lastPage.namespaces.length < 100) {
					return undefined;
				}
				return lastPage.pagination.cursor;
			},
			select: (data) => data.pages.flatMap((page) => page.namespaces),
		});

	const projectsQueryOptions = (opts: { organization: string }) =>
		infiniteQueryOptions({
			queryKey: [opts, "projects"],
			initialPageParam: undefined as string | undefined,
			queryFn: async ({ pageParam }) => {
				const data = await client.projects.list({
					org: opts.organization,
					cursor: pageParam ?? undefined,
					limit: RECORDS_PER_PAGE,
				});
				return data;
			},
			getNextPageParam: (lastPage) => {
				if (lastPage.projects.length < RECORDS_PER_PAGE) {
					return undefined;
				}
				return lastPage.pagination.cursor;
			},
			select: (data) => data.pages.flatMap((page) => page.projects),
		});

	const projectQueryOptions = (opts: {
		project: string;
		organization: string;
	}) =>
		queryOptions({
			queryKey: [opts, "project"],
			queryFn: async () => {
				const data = await client.projects.get(opts.project, {
					org: opts.organization,
				});
				return data.project;
			},
			enabled: !!opts.project,
			...no404Retry(),
		});

	const namespaceAccessTokenQueryOptions = (opts: {
		namespace: string;
		organization: string;
		project: string;
	}) =>
		queryOptions({
			staleTime: 15 * 60 * 1000,
			gcTime: 15 * 60 * 1000,
			retry: false,
			queryKey: [opts, "access-token"] as QueryKey,
			queryFn: async () =>
				await client.namespaces.createAccessToken(
					opts.project,
					opts.namespace,
					{ org: opts.organization },
				),
		});

	const namespaceQueryOptions = (opts: {
		namespace: string;
		organization: string;
		project: string;
	}) => {
		return queryOptions({
			queryKey: [opts, "namespace"] as QueryKey,
			queryFn: async () => {
				const data = await client.namespaces.get(
					opts.project,
					opts.namespace,
					{
						org: opts.organization,
					},
				);
				return data.namespace;
			},
			...no404Retry(),
		});
	};

	const organizationsQueryOptions = () =>
		queryOptions({
			queryKey: ["organizations"],
			queryFn: async () => {
				const result = await authClient.organization.list();
				if (!result.data) {
					throw new Error("Failed to list organizations");
				}
				return result.data;
			},
		});

	const createProjectMutationOptions = () =>
		mutationOptions({
			mutationKey: ["projects", "create"],
			mutationFn: async (data: {
				displayName: string;
				organization: string;
			}) => {
				const response = await client.projects.create({
					displayName: data.displayName,
					org: data.organization,
				});

				return response;
			},
		});

	const billingProjectSubscriptionUpdateSessionQueryOptions = ({
		project,
		organization,
	}: {
		project: string;
		organization: string;
	}) =>
		queryOptions({
			queryKey: [
				{ organization, project },
				"subscription",
				"update-session",
			],
			queryFn: async () => {
				const response =
					await client.billing.createSubscriptionUpdateSession(
						project,
						{
							org: organization,
						},
					);
				return response.url;
			},
		});

	const projectMetricsQueryOptions = (opts: {
		organization: string;
		project: string;
		name: Rivet.MetricName | Rivet.MetricName[];
		startAt?: string;
		endAt?: string;
		resolution?: number;
	}) =>
		queryOptions({
			queryKey: [opts, "metrics"],
			queryFn: async () => {
				const data = await client.projects.metrics.get(opts.project, {
					name: opts.name,
					org: opts.organization,
					startAt: opts.startAt,
					endAt: opts.endAt,
					resolution: opts.resolution,
				});
				return data;
			},
		});

	const projectLatestMetricsQueryOptions = (opts: {
		organization: string;
		project: string;
		name: Rivet.MetricName | Rivet.MetricName[];
		endAt?: string;
	}) =>
		queryOptions({
			queryKey: [opts, "latest-metrics"],
			queryFn: async () => {
				const data = await client.projects.metrics.getLatest(
					opts.project,
					{
						name: opts.name,
						org: opts.organization,
						endAt: opts.endAt,
					},
				);
				const transformed = data.name.map((name, index) => ({
					name: name as Rivet.MetricName,
					ts: data.ts[index],
					value: BigInt(String(data.value[index])),
				}));
				return transformed;
			},
		});

	const namespaceMetricsQueryOptions = (opts: {
		organization: string;
		project: string;
		namespace: string;
		name: Rivet.MetricName | Rivet.MetricName[];
		startAt?: string;
		endAt?: string;
		resolution?: number;
	}) =>
		queryOptions({
			queryKey: [opts, "metrics"],
			queryFn: async () => {
				const data = await client.namespaces.metrics.get(
					opts.project,
					opts.namespace,
					{
						name: opts.name,
						org: opts.organization,
						startAt: opts.startAt,
						endAt: opts.endAt,
						resolution: opts.resolution,
					},
				);
				return data;
			},
		});

	const projectComputeMetricsQueryOptions = (opts: {
		organization: string;
		project: string;
		name: Rivet.ComputeMetricName | Rivet.ComputeMetricName[];
		startAt?: string;
		endAt?: string;
		resolution?: number;
	}) =>
		queryOptions({
			queryKey: [opts, "compute-metrics"],
			queryFn: async () => {
				const data = await client.projects.metrics.getCompute(
					opts.project,
					{
						name: opts.name,
						org: opts.organization,
						startAt: opts.startAt,
						endAt: opts.endAt,
						resolution: opts.resolution,
					},
				);
				return data;
			},
			// A project with no compute pools 404s here. Fail fast instead of
			// retrying so the billing page can drop the usage figure
			// immediately rather than showing a long loading skeleton.
			...no404Retry(),
		});

	const namespaceLatestMetricsQueryOptions = (opts: {
		organization: string;
		project: string;
		namespace: string;
		name: Rivet.MetricName | Rivet.MetricName[];
		endAt?: string;
	}) =>
		queryOptions({
			queryKey: [opts, "latest-metrics"],
			queryFn: async () => {
				const data = await client.namespaces.metrics.getLatest(
					opts.project,
					opts.namespace,
					{
						name: opts.name,
						org: opts.organization,
						endAt: opts.endAt,
					},
				);
				const transformed = data.name.map((name, index) => ({
					name: name as Rivet.MetricName,
					ts: data.ts[index],
					value: BigInt(String(data.value[index])),
				}));
				return transformed;
			},
		});

	const archiveProjectMutationOptions = () => {
		return mutationOptions({
			mutationKey: [{ organization }, "projects", "archive"],
			mutationFn: async (data: { project: string }) => {
				const response = await client.projects.delete(data.project, {
					org: organization,
				});
				return response;
			},
		});
	};

	const archiveNamespaceMutationOptions = () => {
		return mutationOptions({
			mutationKey: [{ organization }, "namespaces", "archive"],
			mutationFn: async (data: {
				project: string;
				namespace: string;
			}) => {
				const response = await client.namespaces.delete(
					data.project,
					data.namespace,
					{
						org: organization,
					},
				);
				return response;
			},
		});
	};

	return {
		...parent,
		client,
		organization,
		organizationsQueryOptions,
		orgProjectNamespacesQueryOptions,
		namespaceAccessTokenQueryOptions,
		currentOrgProjectNamespacesQueryOptions: (opts: {
			project: string;
		}) => {
			return orgProjectNamespacesQueryOptions({
				organization,
				project: opts.project,
			});
		},
		projectsQueryOptions,
		currentOrgProjectsQueryOptions: () => {
			return projectsQueryOptions({ organization });
		},
		currentOrgProjectQueryOptions: (opts: { project: string }) => {
			return projectQueryOptions({ organization, project: opts.project });
		},
		currentOrgProjectNamespaceQueryOptions(opts: {
			project: string;
			namespace: string;
		}) {
			return namespaceQueryOptions({
				organization,
				project: opts.project,
				namespace: opts.namespace,
			});
		},
		currentOrgProjectNamespaceActorNamesQueryOptions(opts: {
			project: string;
			namespace: string;
		}) {
			return infiniteQueryOptions({
				queryKey: [
					{
						organization,
						project: opts.project,
						namespace: opts.namespace,
					},
					"cross-namespace-actor-names",
				] as QueryKey,
				staleTime: 30 * 1000,
				gcTime: 5 * 60 * 1000,
				initialPageParam: undefined as string | undefined,
				queryFn: async ({ pageParam }) => {
					const ns = await queryClient.fetchQuery(
						namespaceQueryOptions({
							organization,
							project: opts.project,
							namespace: opts.namespace,
						}),
					);
					const engineClient = createEngineClient(
						cloudEnv().VITE_APP_API_URL,
						{
							token: async () =>
								(
									await queryClient.fetchQuery(
										namespaceAccessTokenQueryOptions({
											organization,
											project: opts.project,
											namespace: opts.namespace,
										}),
									)
								).token,
						},
					);
					return await engineClient.actorsListNames({
						namespace: ns.access.engineNamespaceName,
						cursor: pageParam ?? undefined,
						limit: RECORDS_PER_PAGE,
					});
				},
				getNextPageParam: (lastPage) => {
					if (Object.keys(lastPage.names).length < RECORDS_PER_PAGE) {
						return undefined;
					}
					return lastPage.pagination?.cursor;
				},
				select: (data) =>
					data.pages.flatMap((page) =>
						Object.entries(page.names).map(([id, name]) => ({
							id,
							name,
						})),
					),
				retry: false,
			});
		},
		createProjectMutationOptions,
		currentOrgCreateProjectMutationOptions() {
			return mutationOptions({
				mutationKey: [{ organization }, "projects", "create"],
				mutationFn: async (data: { displayName: string }) => {
					const response = await client.projects.create({
						displayName: data.displayName,
						org: organization,
					});

					return response;
				},
			});
		},
		billingProjectSubscriptionUpdateSessionQueryOptions,
		currentOrgBillingProjectSubscriptionUpdateSessionQueryOptions(opts: {
			project: string;
		}) {
			return billingProjectSubscriptionUpdateSessionQueryOptions({
				organization,
				project: opts.project,
			});
		},
		currentOrganizationBillingDetailsQueryOptions({
			project,
		}: {
			project: string;
		}) {
			return parent.billingDetailsQueryOptions({
				organization,
				project,
			});
		},
		currentOrganizationProjectMetricsQueryOptions(
			opts: Omit<
				Parameters<typeof projectMetricsQueryOptions>[0],
				"organization"
			>,
		) {
			return projectMetricsQueryOptions({
				organization,
				...opts,
			});
		},
		currentOrganizationProjectLatestMetricsQueryOptions(
			opts: Omit<
				Parameters<typeof projectLatestMetricsQueryOptions>[0],
				"organization"
			>,
		) {
			return projectLatestMetricsQueryOptions({
				organization,
				...opts,
			});
		},
		currentOrganizationNamespaceMetricsQueryOptions(
			opts: Omit<
				Parameters<typeof namespaceMetricsQueryOptions>[0],
				"organization"
			>,
		) {
			return namespaceMetricsQueryOptions({
				organization,
				...opts,
			});
		},
		currentOrganizationProjectComputeMetricsQueryOptions(
			opts: Omit<
				Parameters<typeof projectComputeMetricsQueryOptions>[0],
				"organization"
			>,
		) {
			return projectComputeMetricsQueryOptions({
				organization,
				...opts,
			});
		},
		currentOrganizationNamespaceLatestMetricsQueryOptions(
			opts: Omit<
				Parameters<typeof namespaceLatestMetricsQueryOptions>[0],
				"organization"
			>,
		) {
			return namespaceLatestMetricsQueryOptions({
				organization,
				...opts,
			});
		},
		archiveProjectMutationOptions,
		archiveNamespaceMutationOptions,
		currentOrganizationManagedPoolsQueryOptions(opts: {
			project: string;
			namespace: string;
		}) {
			return parent.managedPoolsQueryOptions({
				organization,
				project: opts.project,
				namespace: opts.namespace,
			});
		},
		currentOrganizationManagedPoolQueryOptions(opts: {
			project: string;
			namespace: string;
			pool: string;
		}) {
			return parent.managedPoolQueryOptions({
				organization,
				project: opts.project,
				namespace: opts.namespace,
				pool: opts.pool,
			});
		},
		currentOrganizationImageRepositoriesQueryOptions(opts: {
			project: string;
		}) {
			return parent.imageRepositoriesQueryOptions({
				organization,
				project: opts.project,
			});
		},
		currentOrganizationTagsQueryOptions(opts: {
			project: string;
			repository: string;
		}) {
			return parent.tagsQueryOptions({
				organization,
				project: opts.project,
				repository: opts.repository,
			});
		},
		// currentOrganizationImagesQueryOptions(opts: { project: string }) {
		// 	return parent.imagesQueryOptions({
		// 		organization,
		// 		project: opts.project,
		// 	});
		// },
	};
};

export const createProjectContext = ({
	client,
	organization,
	project,
	...parent
}: {
	client: RivetClient;
	organization: string;
	project: string;
} & ReturnType<typeof createOrganizationContext> &
	ReturnType<typeof createGlobalContext>) => {
	return {
		...parent,
		client,
		organization,
		project,
		createNamespaceMutationOptions(opts: {
			onSuccess?: (data: Namespace) => void;
		}) {
			return {
				...opts,
				mutationKey: ["namespaces"],
				mutationFn: async (data: CreateNamespace) => {
					const response = await client.namespaces.create(project, {
						displayName: data.displayName,
						org: organization,
					});
					return {
						id: response.namespace.id,
						name: response.namespace.name,
						displayName: response.namespace.displayName,
						createdAt: new Date(
							response.namespace.createdAt,
						).toISOString(),
					};
				},
			};
		},
		currentProjectQueryOptions: () => {
			return parent.currentOrgProjectQueryOptions({
				project,
			});
		},
		currentProjectNamespacesQueryOptions: () => {
			return parent.orgProjectNamespacesQueryOptions({
				organization,
				project,
			});
		},
		namespacesQueryOptions() {
			return parent.orgProjectNamespacesQueryOptions({
				organization,
				project,
			});
		},
		currentProjectNamespaceQueryOptions(opts: { namespace: string }) {
			return parent.currentOrgProjectNamespaceQueryOptions({
				project,
				namespace: opts.namespace,
			});
		},
		currentProjectBillingDetailsQueryOptions() {
			return parent.currentOrganizationBillingDetailsQueryOptions({
				project,
			});
		},
		currentProjectBillingUsageQueryOptions() {
			return parent.billingUsageQueryOptions({
				organization,
				project,
			});
		},
		changeCurrentProjectBillingPlanMutationOptions() {
			return {
				mutationKey: [{ organization, project }, "billing"],
				mutationFn: async (
					data: Rivet.BillingSetPlanRequest & {
						__from?: Rivet.BillingPlan;
					},
				) => {
					const response = await client.billing.setPlan(project, {
						plan: data.plan,
						org: organization,
					});
					return response;
				},
			};
		},
		accessTokenQueryOptions({ namespace }: { namespace: string }) {
			return parent.namespaceAccessTokenQueryOptions({
				organization,
				project,
				namespace,
			});
		},
		// API Token methods
		createApiTokenQueryOptions({ name }: { name: string }) {
			return queryOptions({
				staleTime: Number.POSITIVE_INFINITY,
				queryKey: [{ organization, project }, "api-tokens", name],
				queryFn: async () => {
					const response = await client.apiTokens.create(project, {
						name: name,
						org: organization,
					});
					return response.apiToken;
				},
			});
		},
		apiTokensQueryOptions() {
			return queryOptions({
				queryKey: [{ organization, project }, "api-tokens"],
				queryFn: async () => {
					const response = await client.apiTokens.list(project, {
						org: organization,
					});
					return response;
				},
			});
		},
		createApiTokenMutationOptions(opts?: {
			onSuccess?: (data: Rivet.ApiTokensCreateRequest) => void;
		}) {
			return {
				mutationKey: [
					{ organization, project },
					"api-tokens",
					"create",
				],
				mutationFn: async (data: {
					name: string;
					expiresAt?: string;
				}) => {
					const response = await client.apiTokens.create(project, {
						name: data.name,
						expiresAt: data.expiresAt,
						org: organization,
					});
					return response;
				},
				onSuccess: opts?.onSuccess,
			};
		},
		revokeApiTokenMutationOptions(opts?: { onSuccess?: () => void }) {
			return {
				mutationKey: [
					{ organization, project },
					"api-tokens",
					"revoke",
				],
				mutationFn: async (data: { apiTokenId: string }) => {
					const response = await client.apiTokens.revoke(
						project,
						data.apiTokenId,
						{ org: organization },
					);
					return response;
				},
				onSuccess: opts?.onSuccess,
			};
		},
		currentProjectBillingSubscriptionUpdateSessionQueryOptions() {
			return parent.billingProjectSubscriptionUpdateSessionQueryOptions({
				organization,
				project,
			});
		},
		currentProjectMetricsQueryOptions(
			opts: Omit<
				Parameters<
					typeof parent.currentOrganizationProjectMetricsQueryOptions
				>[0],
				"project"
			>,
		) {
			return parent.currentOrganizationProjectMetricsQueryOptions({
				project,
				...opts,
			});
		},
		currentProjectLatestMetricsQueryOptions(
			opts: Omit<
				Parameters<
					typeof parent.currentOrganizationProjectLatestMetricsQueryOptions
				>[0],
				"project"
			>,
		) {
			return parent.currentOrganizationProjectLatestMetricsQueryOptions({
				project,
				...opts,
			});
		},
		currentProjectNamespaceMetricsQueryOptions(
			opts: Omit<
				Parameters<
					typeof parent.currentOrganizationNamespaceMetricsQueryOptions
				>[0],
				"project"
			>,
		) {
			return parent.currentOrganizationNamespaceMetricsQueryOptions({
				project,
				...opts,
			});
		},
		currentProjectComputeMetricsQueryOptions(
			opts: Omit<
				Parameters<
					typeof parent.currentOrganizationProjectComputeMetricsQueryOptions
				>[0],
				"project"
			>,
		) {
			return parent.currentOrganizationProjectComputeMetricsQueryOptions({
				project,
				...opts,
			});
		},
		currentProjectNamespaceLatestMetricsQueryOptions(
			opts: Omit<
				Parameters<
					typeof parent.currentOrganizationNamespaceLatestMetricsQueryOptions
				>[0],
				"project"
			>,
		) {
			return parent.currentOrganizationNamespaceLatestMetricsQueryOptions(
				{
					project,
					...opts,
				},
			);
		},
		archiveCurrentProjectMutationOptions() {
			return mutationOptions({
				mutationKey: parent.archiveProjectMutationOptions().mutationKey,
				mutationFn: async () => {
					const response = await client.projects.delete(project, {
						org: organization,
					});
					return response;
				},
			});
		},
		currentProjectManagedPoolsQueryOptions(opts: { namespace: string }) {
			return parent.managedPoolsQueryOptions({
				organization,
				project,
				namespace: opts.namespace,
			});
		},
		currentProjectManagedPoolQueryOptions(opts: {
			namespace: string;
			pool: string;
			safe?: boolean;
		}) {
			return parent.managedPoolQueryOptions({
				organization,
				project,
				namespace: opts.namespace,
				pool: opts.pool,
				safe: opts.safe,
			});
		},
		currentProjectImageRepositoriesQueryOptions() {
			return parent.imageRepositoriesQueryOptions({
				organization,
				project,
			});
		},
		currentProjectTagsQueryOptions(opts: { repository: string }) {
			return parent.tagsQueryOptions({
				organization,
				project,
				repository: opts.repository,
			});
		},
		currentProjectImagesQueryOptions(opts?: { limit?: number }) {
			return parent.imagesQueryOptions({
				organization,
				project,
				limit: opts?.limit,
			});
		},
		// Polls the project image registry. Used by the onboarding frontend
		// step to hold back the "Waiting for an Actor" caption until the user
		// has pushed their first image to Rivet Compute.
		currentProjectFirstImagePresentQueryOptions() {
			return queryOptions({
				queryKey: [
					organization,
					project,
					"images",
					"first-present",
				] as QueryKey,
				queryFn: async () => {
					const response = await client.docker.listImages(project, {
						limit: 1,
						org: organization,
					});
					return response.images.length > 0;
				},
				refetchInterval: 5000,
			});
		},
		upsertCurrentProjectManagedPoolMutationOptions() {
			return mutationOptions({
				mutationKey: [organization, project, "managed-pool", "upsert"],
				mutationFn: async ({
					namespace,
					pool,
					...request
				}: Rivet.ManagedPoolsUpsertRequest & {
					namespace: string;
					pool: string;
				}) => {
					return await client.managedPools.upsert(
						project,
						namespace,
						pool,
						{ ...request, org: organization },
					);
				},
			});
		},
		deleteCurrentProjectManagedPoolMutationOptions() {
			return mutationOptions({
				mutationKey: [organization, project, "managed-pool", "delete"],
				mutationFn: async ({
					namespace,
					pool,
				}: {
					namespace: string;
					pool: string;
				}) => {
					return await client.managedPools.delete(
						project,
						namespace,
						pool,
						{ org: organization },
					);
				},
			});
		},
	};
};

export const createNamespaceContext = ({
	namespace,
	engineNamespaceName,
	engineNamespaceId,
	...parent
}: {
	namespace: string;
	engineNamespaceName: string;
	engineNamespaceId: string;
} & ReturnType<typeof createProjectContext> &
	ReturnType<typeof createOrganizationContext> &
	ReturnType<typeof createGlobalContext>) => {
	const token = async () => {
		const response = await queryClient.fetchQuery(
			parent.accessTokenQueryOptions({ namespace }),
		);

		return response.token;
	};
	return {
		...parent,
		cloudNamespace: namespace,
		...createEngineNamespaceContext({
			...parent,
			namespace: engineNamespaceName,
			engineToken: token,
			client: createEngineClient(cloudEnv().VITE_APP_API_URL, {
				token,
			}),
			namespacesQueryOptions() {
				return parent.currentProjectNamespacesQueryOptions();
			},
			namespaceQueryOptions(
				name: string | undefined,
			): ReturnType<
				ReturnType<
					typeof createEngineNamespaceContext
				>["namespaceQueryOptions"]
			> {
				return parent.currentProjectNamespaceQueryOptions({
					namespace: name ?? namespace,
				}) as ReturnType<
					ReturnType<
						typeof createEngineNamespaceContext
					>["namespaceQueryOptions"]
				>;
			},
		}),
		currentNamespaceAccessTokenQueryOptions() {
			return parent.accessTokenQueryOptions({ namespace });
		},
		engineAdminTokenQueryOptions(): UseQueryOptions<string> {
			return queryOptions({
				staleTime: 5 * 60 * 1000, // 5 minutes
				gcTime: 5 * 60 * 1000, // 5 minutes
				queryKey: [
					{
						namespace,
						project: parent.project,
						organization: parent.organization,
					},
					"tokens",
					"engine-admin",
				] as QueryKey,
				queryFn: async () => {
					const f = parent.client.namespaces.createSecretToken(
						parent.project,
						namespace,
						{ org: parent.organization },
					);
					const t = await f;
					return t.token;
				},
			});
		},
		connectionTokenQueryOptions() {
			return queryOptions({
				staleTime: 5 * 60 * 1000, // 5 minutes
				gcTime: 5 * 60 * 1000, // 5 minutes
				queryKey: [
					{
						namespace,
						project: parent.project,
						organization: parent.organization,
					},
					"tokens",
					"connection",
				],
				queryFn: async () => {
					// The endpoint is get-or-create: it returns the
					// namespace's existing connection token and only mints
					// one on the first call.
					return await parent.client.namespaces.createConnectionToken(
						parent.project,
						namespace,
						{ org: parent.organization },
					);
				},
			});
		},
		publishableTokenQueryOptions() {
			return queryOptions({
				staleTime: 5 * 60 * 1000, // 5 minutes
				gcTime: 5 * 60 * 1000, // 5 minutes
				queryKey: [
					{
						namespace,
						project: parent.project,
						organization: parent.organization,
					},
					"tokens",
					"publishable",
				],
				queryFn: async () => {
					const f = parent.client.namespaces.createPublishableToken(
						parent.project,
						namespace,
						{ org: parent.organization },
					);
					const t = await f;
					return t.token as string;
				},
			});
		},
		currentNamespaceQueryOptions() {
			return parent.currentProjectNamespaceQueryOptions({ namespace });
		},
		currentNamespaceMetricsQueryOptions(
			opts: Omit<
				Parameters<
					typeof parent.currentProjectNamespaceMetricsQueryOptions
				>[0],
				"namespace"
			>,
		) {
			return parent.currentProjectNamespaceMetricsQueryOptions({
				namespace,
				...opts,
			});
		},
		currentNamespaceLatestMetricsQueryOptions(
			opts: Omit<
				Parameters<
					typeof parent.currentProjectNamespaceLatestMetricsQueryOptions
				>[0],
				"namespace"
			>,
		) {
			return parent.currentProjectNamespaceLatestMetricsQueryOptions({
				namespace,
				...opts,
			});
		},
		archiveCurrentNamespaceMutationOptions() {
			return mutationOptions({
				mutationKey:
					parent.archiveNamespaceMutationOptions().mutationKey,
				mutationFn: async () => {
					const response = await parent.client.namespaces.delete(
						parent.project,
						namespace,
						{ org: parent.organization },
					);
					return response;
				},
			});
		},

		currentNamespaceManagedPoolsQueryOptions() {
			return parent.currentProjectManagedPoolsQueryOptions({
				namespace,
			});
		},

		currentNamespaceManagedPoolQueryOptions(opts: {
			pool: string;
			safe?: boolean;
		}) {
			return parent.currentProjectManagedPoolQueryOptions({
				namespace,
				pool: opts.pool,
				safe: opts.safe,
			});
		},

		currentNamespaceHasManagedPoolQueryOptions(
			opts: { pool?: string } = {},
		) {
			return queryOptions({
				...parent.currentProjectManagedPoolQueryOptions({
					namespace,
					pool: opts.pool ?? "default",
					safe: true,
				}),
				select: (data) => !!data?.config?.image,
			});
		},

		upsertCurrentNamespaceManagedPoolMutationOptions() {
			return mutationOptions({
				mutationKey:
					parent.upsertCurrentProjectManagedPoolMutationOptions()
						.mutationKey,
				mutationFn: async ({
					pool,
					...request
				}: Omit<
					Rivet.ManagedPoolsUpsertRequest,
					"project" | "namespace" | "org"
				> & { pool: string }) => {
					return await parent.client.managedPools.upsert(
						parent.project,
						namespace,
						pool,
						{ ...request, org: parent.organization },
					);
				},
			});
		},

		deleteCurrentNamespaceManagedPoolMutationOptions() {
			return mutationOptions({
				mutationKey:
					parent.deleteCurrentProjectManagedPoolMutationOptions()
						.mutationKey,
				mutationFn: async ({ pool }: { pool: string }) => {
					return await parent.client.managedPools.delete(
						parent.project,
						namespace,
						pool,
						{ org: parent.organization },
					);
				},
			});
		},
	};
};
