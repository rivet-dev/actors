import { faServer, Icon } from "@rivet-gg/icons";
import { useState } from "react";
import { type DialogContentProps, Frame } from "@/components";
import { RunnerConfigToggleGroup } from "../runner-config-toggle-group";
import ConnectManualServerfulFrameContent from "./connect-manual-serverful-frame";
import ConnectManualServerlessFrameContent from "./connect-manual-serverless-frame";

interface CreateProjectFrameContentProps extends DialogContentProps {}

export default function CreateProjectFrameContent({
	onClose,
}: CreateProjectFrameContentProps) {
	const [mode, setMode] = useState("serverful");

	return (
		<>
			<Frame.Header>
				<Frame.Title className="gap-2 flex items-center">
					<div>
						Add <Icon icon={faServer} className="ml-0.5" /> Custom
					</div>
				</Frame.Title>
			</Frame.Header>
			<Frame.Content>
				<RunnerConfigToggleGroup mode={mode} onChange={setMode} />
				{mode === "serverless" ? (
					<ConnectManualServerlessFrameContent
						provider="custom"
						onClose={onClose}
					/>
				) : null}
				{mode === "serverful" ? (
					<ConnectManualServerfulFrameContent
						onClose={onClose}
						provider="custom"
					/>
				) : null}
			</Frame.Content>
		</>
	);
}
