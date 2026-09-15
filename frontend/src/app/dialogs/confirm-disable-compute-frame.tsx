import { useMutation } from "@tanstack/react-query";
import { useState } from "react";
import { Button, type DialogContentProps, Frame } from "@/components";
import { useCloudProjectDataProvider } from "@/components/actors";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { posthog } from "@/lib/posthog";
import { queryClient } from "@/queries/global";

interface ConfirmDisableComputeContentProps extends DialogContentProps {
	slug: string;
}

export default function ConfirmDisableComputeContent({
	onClose,
	slug,
}: ConfirmDisableComputeContentProps) {
	const dataProvider = useCloudProjectDataProvider();
	const [confirmValue, setConfirmValue] = useState("");

	const isConfirmed = confirmValue === slug;

	const { mutate, isPending } = useMutation({
		...dataProvider.disableComputeMutationOptions(),
		onSuccess: async ({ deletedPools }) => {
			posthog.capture("project_compute_disabled", {
				slug,
				deletedPools,
			});
			// Pools were destroyed across every namespace, so drop all cached
			// pool/namespace queries and let the affected surfaces refetch.
			queryClient.invalidateQueries();
			onClose?.();
		},
	});

	return (
		<>
			<Frame.Header>
				<Frame.Title className="gap-2 flex items-center">
					<div>Disable Rivet Compute for '{slug}'</div>
				</Frame.Title>
				<Frame.Description>
					This destroys every Rivet Compute deployment across all
					namespaces in this project. Running Rivet Actors on those
					pools become unreachable, their runners are torn down, and
					deployment tokens are revoked. This action cannot be undone.
					You can re-enable Compute later by deploying again.
				</Frame.Description>
			</Frame.Header>
			<Frame.Content>
				<div className="space-y-2">
					<Label htmlFor="confirm-disable-compute">
						Type <span className="font-bold">{slug}</span> to confirm
					</Label>
					<Input
						id="confirm-disable-compute"
						value={confirmValue}
						onChange={(e) => setConfirmValue(e.target.value)}
						placeholder={slug}
					/>
				</div>
			</Frame.Content>
			<Frame.Footer>
				<Button
					variant="destructive"
					isLoading={isPending}
					disabled={!isConfirmed}
					onClick={() => {
						mutate(undefined);
					}}
				>
					Disable Compute
				</Button>
				<Button variant="secondary" onClick={onClose}>
					Cancel
				</Button>
			</Frame.Footer>
		</>
	);
}
