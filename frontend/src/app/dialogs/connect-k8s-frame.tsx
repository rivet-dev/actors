import { faKubernetes, Icon } from "@rivet-gg/icons";
import { type DialogContentProps, Frame } from "@/components";
import ConnectManualServerfulFrameContent from "./connect-manual-serverful-frame";

interface ConnectK8sFrameContentProps extends DialogContentProps {
	footer?: React.ReactNode;
	title?: React.ReactNode;
}

export default function ConnectK8sFrameContent({
	onClose,
	footer,
	title,
}: ConnectK8sFrameContentProps) {
	return (
		<>
			<Frame.Header>
				<Frame.Title className="gap-2 flex items-center">
					{title ?? (
						<div>
							Add <Icon icon={faKubernetes} className="ml-0.5" />{" "}
							Kubernetes
						</div>
					)}
				</Frame.Title>
			</Frame.Header>
			<Frame.Content>
				<ConnectManualServerfulFrameContent
					provider="kubernetes"
					onClose={onClose}
					footer={footer}
				/>
			</Frame.Content>
		</>
	);
}
