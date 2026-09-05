import { faXmark, Icon } from "@rivet-gg/icons";
import { useMutation } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { useFormContext } from "react-hook-form";
import z from "zod";
import {
	Button,
	Card,
	CardContent,
	CardDescription,
	CardFooter,
	CardHeader,
	CardTitle,
	cn,
	createSchemaForm,
	FormControl,
	FormField,
	FormItem,
	FormLabel,
	FormMessage,
	Input,
} from "@/components";
import { authClient } from "@/lib/auth";
import { paletteForLetter } from "@/lib/org-palette";
import { queryClient } from "@/queries/global";

const formSchema = z.object({
	name: z.string().nonempty("Name cannot be empty"),
});
type FormValues = z.infer<typeof formSchema>;

const { Form, Submit } = createSchemaForm(formSchema);

export function NewOrgPage() {
	const navigate = useNavigate();
	const [nameDraft, setNameDraft] = useState("");
	const palette = useMemo(() => paletteForLetter(nameDraft), [nameDraft]);

	const { mutateAsync, isPending } = useMutation({
		mutationFn: async (values: { name: string }) => {
			const result = await authClient.organization.create({
				name: values.name,
				slug: crypto.randomUUID(),
			});
			if (result.error) throw result.error;
			return result;
		},
		onSuccess: async (data) => {
			await queryClient.invalidateQueries({
				queryKey: ["organizations"],
			});
			await navigate({
				to: "/orgs/$organization",
				params: { organization: data.data.slug },
			});
		},
	});

	return (
		<div
			className="relative flex min-h-screen w-full flex-col bg-background text-foreground"
			style={
				{
					"--org-c1": palette.c1,
					"--org-c2": palette.c2,
					"--org-c3": palette.c3,
					"--org-c4": palette.c4,
					"--org-accent": palette.accent,
				} as React.CSSProperties
			}
		>
			<div className="absolute right-4 top-4 z-10">
				<Button
					type="button"
					variant="ghost"
					size="icon"
					onClick={() => navigate({ to: "/" })}
					aria-label="Close"
				>
					<Icon icon={faXmark} className="size-4" />
				</Button>
			</div>

			<div className="flex flex-1 items-center justify-center px-6">
				<Card className="w-full max-w-md">
					<Form
						defaultValues={{ name: "" }}
						mode="onSubmit"
						revalidateMode="onSubmit"
						onSubmit={async (values, form) => {
							try {
								await mutateAsync(values);
							} catch {
								form.setError("root", {
									message:
										"Couldn't create the organization. Try a different name.",
								});
							}
						}}
					>
						<CardHeader>
							<CardTitle>Create a new organization</CardTitle>
							<CardDescription>
								Organizations are shared workspaces where
								teammates collaborate across projects.
							</CardDescription>
						</CardHeader>
						<CardContent className="flex flex-col gap-6">
							<div className="flex justify-center">
								<GradientAvatar
									palette={palette}
									letter={
										nameDraft.trim()[0]?.toUpperCase() ?? ""
									}
								/>
							</div>
							<NameField onValueChange={setNameDraft} />
							<RootError />
						</CardContent>
						<CardFooter className="justify-end">
							<Submit type="submit" isLoading={isPending}>
								Continue
							</Submit>
						</CardFooter>
					</Form>
				</Card>
			</div>
		</div>
	);
}

function GradientAvatar({
	palette,
	letter,
}: {
	palette: ReturnType<typeof paletteForLetter>;
	letter: string;
}) {
	return (
		<div className="relative h-16 w-16 animate-orb-breathe">
			<div className="absolute inset-0 overflow-hidden rounded-full [clip-path:circle(50%)]">
				<div
					className="absolute -inset-2 animate-conic-rotate"
					style={{
						background: `conic-gradient(from var(--gradient-angle, 0deg) at 50% 50%, ${palette.c1}, ${palette.c2}, ${palette.c3}, ${palette.c4}, ${palette.c2}, ${palette.c1})`,
						filter: "blur(4px) saturate(0.95) contrast(1.05)",
						transition: "background 600ms ease",
					}}
				/>
				<div
					className="absolute -inset-2 animate-conic-rotate-reverse"
					style={{
						background: `conic-gradient(from var(--gradient-angle, 0deg) at 50% 50%, transparent, ${palette.accent}, transparent, ${palette.c4}, transparent)`,
						filter: "blur(10px)",
						mixBlendMode: "overlay",
						opacity: 0.55,
						transition: "background 600ms ease",
					}}
				/>
				<div
					className="absolute inset-0 pointer-events-none"
					style={{
						background:
							"radial-gradient(circle at 30% 22%, hsl(0 0% 100% / 0.4) 0%, hsl(0 0% 100% / 0) 38%), radial-gradient(circle at 72% 82%, hsl(0 0% 0% / 0.45) 0%, hsl(0 0% 0% / 0) 65%)",
					}}
				/>
			</div>
			{letter ? (
				<div className="absolute inset-0 flex items-center justify-center pointer-events-none">
					<span className="text-2xl font-semibold text-white">
						{letter}
					</span>
				</div>
			) : null}
		</div>
	);
}

function NameField({
	onValueChange,
}: {
	onValueChange: (value: string) => void;
}) {
	const { control } = useFormContext<FormValues>();
	return (
		<FormField
			control={control}
			name="name"
			render={({ field }) => (
				<FormItem className="space-y-1.5">
					<FormLabel className="text-xs font-normal text-muted-foreground">
						Organization name
					</FormLabel>
					<FormControl>
						<Input
							placeholder="Acme"
							autoFocus
							autoComplete="off"
							{...field}
							onChange={(e) => {
								field.onChange(e);
								onValueChange(e.target.value);
							}}
							className={cn("h-10 text-sm")}
						/>
					</FormControl>
					<FormMessage />
				</FormItem>
			)}
		/>
	);
}

function RootError() {
	const { formState } = useFormContext<FormValues>();
	if (!formState.errors.root) return null;
	return (
		<p className="text-sm text-destructive">
			{formState.errors.root.message}
		</p>
	);
}
