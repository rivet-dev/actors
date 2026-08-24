import { faRightFromBracket, faUserSecret, Icon } from "@rivet-gg/icons";
import { useMutation } from "@tanstack/react-query";
import { Button } from "@/components";
import { authClient } from "@/lib/auth";
import { queryClient } from "@/queries/global";

export function ImpersonationBanner() {
	const { data: session } = authClient.useSession();

	const stopImpersonating = useMutation({
		mutationFn: async () => {
			const result = await authClient.admin.stopImpersonating();
			if (result.error) {
				throw new Error(
					result.error.message ?? "Failed to stop impersonating",
				);
			}
		},
		onSuccess: () => {
			queryClient.clear();
			window.location.href = "/";
		},
	});

	if (!session?.session?.impersonatedBy) return null;

	return (
		<div className="fixed inset-x-0 bottom-0 z-50 flex items-center justify-center gap-3 border-t border-destructive/40 bg-destructive px-4 py-2 text-sm text-destructive-foreground">
			<Icon icon={faUserSecret} className="size-4" />
			<span>
				You are impersonating{" "}
				<strong>{session.user?.email ?? "another user"}</strong>.
			</span>
			<Button
				variant="secondary"
				size="sm"
				isLoading={stopImpersonating.isPending}
				startIcon={<Icon icon={faRightFromBracket} />}
				onClick={() => stopImpersonating.mutate()}
			>
				Stop impersonating
			</Button>
		</div>
	);
}
