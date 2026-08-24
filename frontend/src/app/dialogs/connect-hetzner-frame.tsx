import { faHetznerH, Icon } from "@rivet-gg/icons";
import { type DialogContentProps, Frame } from "@/components";
import ConnectManualServerfulFrameContent from "./connect-manual-serverful-frame";

interface ConnectHetznerFrameContentProps extends DialogContentProps {
	title?: React.ReactNode;
}

export default function ConnectHetznerFrameContent({
	onClose,
	title,
}: ConnectHetznerFrameContentProps) {
	return (
		<>
			<Frame.Header>
				<Frame.Title className="gap-2 flex items-center">
					{title ?? (
						<div>
							Add <Icon icon={faHetznerH} className="ml-0.5" />{" "}
							Hetzner
						</div>
					)}
				</Frame.Title>
			</Frame.Header>
			<Frame.Content>
				<ConnectManualServerfulFrameContent
					provider="hetzner"
					onClose={onClose}
				/>
			</Frame.Content>
		</>
	);
}
