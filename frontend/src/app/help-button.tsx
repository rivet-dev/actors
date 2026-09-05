import { faArrowUpRightFromSquare, Icon } from "@rivet-gg/icons";
import { useNavigate, useSearch } from "@tanstack/react-router";
import { useState } from "react";
import {
	Button,
	cn,
	Input,
	Popover,
	PopoverContent,
	PopoverTrigger,
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
	Textarea,
	toast,
} from "@/components";
import { FEEDBACK_FORM_ID } from "@/components/lib/constants";
import { posthog } from "@/lib/posthog";

type Impact = "general" | "minor" | "major" | "critical";

const IMPACT_LABELS: Record<Impact, string> = {
	general: "General question",
	minor: "Minor — non-blocking issue",
	major: "Major — blocking my work",
	critical: "Critical — production down",
};

export function HelpButton({ source = "web" }: { source?: string }) {
	const navigate = useNavigate();
	const search = useSearch({ strict: false }) as Record<string, unknown>;
	const open = search.modal === "help";
	const setOpen = (next: boolean) => {
		void navigate({
			to: ".",
			search: (prev) => ({
				...(prev as Record<string, unknown>),
				modal: next ? "help" : undefined,
			}),
		});
	};
	const [subject, setSubject] = useState("");
	const [message, setMessage] = useState("");
	const [impact, setImpact] = useState<Impact>("general");
	const [submitting, setSubmitting] = useState(false);

	const canSend = subject.trim().length > 0 && message.trim().length > 0;

	const submit = async () => {
		if (!canSend || submitting) return;
		setSubmitting(true);
		try {
			posthog.capture("survey sent", {
				utm_source: source,
				$survey_id: FEEDBACK_FORM_ID,
				$survey_response: `support[${impact}] from ${source}: ${subject.trim()} — ${message.trim()}`,
			});
			setSubject("");
			setMessage("");
			setImpact("general");
			setOpen(false);
			toast.success("Support request sent.");
		} finally {
			setSubmitting(false);
		}
	};

	return (
		<Popover open={open} onOpenChange={setOpen}>
			<PopoverTrigger asChild>
				<Button
					variant="ghost"
					size="sm"
					className={cn(
						"text-muted-foreground hover:text-foreground",
						open && "bg-foreground/[0.06] text-foreground",
					)}
				>
					Help
				</Button>
			</PopoverTrigger>
			<PopoverContent
				align="end"
				className="w-[28rem] p-0"
				onOpenAutoFocus={(e) => e.preventDefault()}
			>
				<div className="flex items-center justify-between px-4 py-2.5 border-b border-foreground/10">
					<h3 className="text-sm font-semibold text-foreground">
						Contact support
					</h3>
					<a
						href="https://rivet.betteruptime.com/"
						target="_blank"
						rel="noopener noreferrer"
						className="inline-flex items-center gap-1.5 text-[11px] text-muted-foreground hover:text-foreground transition-colors"
					>
						Status page
						<Icon
							icon={faArrowUpRightFromSquare}
							className="size-2.5"
						/>
					</a>
				</div>
				<div className="p-3 space-y-3">
					<div className="space-y-1.5">
						<label
							htmlFor="help-subject"
							className="block text-xs font-medium text-foreground"
						>
							Subject
						</label>
						<Input
							id="help-subject"
							value={subject}
							onChange={(e) => setSubject(e.target.value)}
							placeholder="Briefly describe the issue"
							className="h-9 text-sm"
							autoFocus
						/>
					</div>
					<div className="space-y-1.5">
						<label
							htmlFor="help-message"
							className="block text-xs font-medium text-foreground"
						>
							Message
						</label>
						<Textarea
							id="help-message"
							value={message}
							onChange={(e) => setMessage(e.target.value)}
							placeholder="Provide as much detail as possible..."
							className="min-h-[120px] resize-none text-sm"
							onKeyDown={(e) => {
								if (
									(e.metaKey || e.ctrlKey) &&
									e.key === "Enter"
								) {
									e.preventDefault();
									submit();
								}
							}}
						/>
					</div>
					<div className="space-y-1.5">
						<span className="block text-xs font-medium text-foreground">
							Impact
						</span>
						<Select
							value={impact}
							onValueChange={(v) => setImpact(v as Impact)}
						>
							<SelectTrigger className="h-9 text-sm">
								<SelectValue />
							</SelectTrigger>
							<SelectContent className="z-[80]">
								{(Object.keys(IMPACT_LABELS) as Impact[]).map(
									(k) => (
										<SelectItem key={k} value={k}>
											{IMPACT_LABELS[k]}
										</SelectItem>
									),
								)}
							</SelectContent>
						</Select>
					</div>
				</div>
				<div className="flex items-center justify-between gap-3 px-4 pb-3">
					<p className="text-xs text-muted-foreground">
						Prefer the community?{" "}
						<a
							href="https://rivet.dev/discord"
							target="_blank"
							rel="noopener noreferrer"
							className="text-foreground underline-offset-2 hover:underline"
						>
							Discord
						</a>{" "}
						·{" "}
						<a
							href="https://github.com/rivet-dev/rivet"
							target="_blank"
							rel="noopener noreferrer"
							className="text-foreground underline-offset-2 hover:underline"
						>
							GitHub
						</a>
					</p>
					<Button
						type="button"
						size="sm"
						variant="secondary"
						isLoading={submitting}
						disabled={!canSend}
						onClick={submit}
					>
						Send
					</Button>
				</div>
			</PopoverContent>
		</Popover>
	);
}
