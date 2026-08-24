import { type UseFormReturn, useFormContext } from "react-hook-form";
import z from "zod";
import {
	createSchemaForm,
	FormControl,
	FormField,
	FormItem,
	FormLabel,
	FormMessage,
	Input,
} from "@/components";
import { convertStringToId } from "@/lib/utils";

export const formSchema = z.object({
	name: z
		.string()
		.max(128)
		.refine((value) => value.trim() !== "" && value.trim() === value, {
			message: "Name cannot be empty or contain whitespaces",
		}),
	slug: z.string().max(16).optional(),
});

export type FormValues = z.infer<typeof formSchema>;
export type SubmitHandler = (
	values: FormValues,
	form: UseFormReturn<FormValues>,
) => Promise<void>;

const { Form, Submit, SetValue } = createSchemaForm(formSchema);
export { Form, Submit, SetValue };

export const Name = ({ className }: { className?: string }) => {
	const { control } = useFormContext<FormValues>();
	return (
		<FormField
			control={control}
			name="name"
			render={({ field }) => (
				<FormItem className={className}>
					<FormLabel className="col-span-1">Name</FormLabel>
					<FormControl className="row-start-2">
						<Input
							placeholder="Enter a namespace name..."
							maxLength={128}
							{...field}
						/>
					</FormControl>
					<FormMessage className="col-span-1" />
				</FormItem>
			)}
		/>
	);
};

export const Slug = ({ className }: { className?: string }) => {
	const { control, watch } = useFormContext<FormValues>();

	const name = watch("name");

	return (
		<FormField
			control={control}
			name="slug"
			render={({ field }) => (
				<FormItem className={className}>
					<FormLabel className="col-span-2">Slug</FormLabel>
					<FormControl className="row-start-2">
						<Input
							placeholder={
								name
									? convertStringToId(name)
									: "Enter a slug..."
							}
							maxLength={25}
							{...field}
							onChange={(event) => {
								const value = convertStringToId(
									event.target.value,
								);
								field.onChange({ target: { value } });
							}}
						/>
					</FormControl>
					<FormMessage className="col-span-2" />
				</FormItem>
			)}
		/>
	);
};
