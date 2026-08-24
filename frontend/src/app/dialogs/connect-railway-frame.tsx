import { faRailway, Icon } from "@rivet-gg/icons";
import { type DialogContentProps, Frame } from "@/components";
import ConnectManualServerfulFrameContent from "./connect-manual-serverful-frame";

interface ConnectRailwayFrameContentProps extends DialogContentProps {}

export default function ConnectRailwayFrameContent({
	onClose,
}: ConnectRailwayFrameContentProps) {
	return (
		<>
			<Frame.Header>
				<Frame.Title className="gap-2 flex items-center">
					<div>
						Add <Icon icon={faRailway} className="ml-0.5" /> Railway
					</div>
				</Frame.Title>
			</Frame.Header>
			<Frame.Content>
				<ConnectManualServerfulFrameContent
					provider="railway"
					onClose={onClose}
				/>
			</Frame.Content>
		</>
	);
}
