import { cn, ToggleGroup, ToggleGroupItem } from "@/components";

export function RunnerConfigToggleGroup({
	mode,
	id,
	onChange,
	className,
}: {
	id?: string;
	mode: string;
	onChange: (mode: string) => void;
	className?: string;
}) {
	return (
		<div
			id={id}
			className={cn(
				"flex mx-auto items-center justify-center mb-4",
				className,
			)}
		>
			<ToggleGroup
				defaultValue="serverful"
				type="single"
				className="border rounded-md gap-0 w-full"
				value={mode}
				onValueChange={(mode) => {
					if (!mode) {
						return;
					}
					onChange(mode);
				}}
			>
				<ToggleGroupItem
					value="serverless"
					className="rounded-none w-full"
				>
					Serverless
				</ToggleGroupItem>
				<ToggleGroupItem
					value="serverful"
					className="border-l rounded-none w-full"
				>
					Runner
				</ToggleGroupItem>
			</ToggleGroup>
		</div>
	);
}
