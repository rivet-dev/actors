import { useMutation } from "@tanstack/react-query";
import { useNavigate, useSearch } from "@tanstack/react-router";
import type { DialogContentProps } from "@/components/hooks";
import {
	Accordion,
	AccordionContent,
	AccordionItem,
	AccordionTrigger,
} from "@/components/ui/accordion";
import { features } from "@/lib/features";
import { getRandomKey } from "@/lib/words";
import { queryClient } from "@/queries/global";
import {
	DialogDescription,
	DialogFooter,
	DialogHeader,
	DialogTitle,
} from "../../ui/dialog";
import { Flex } from "../../ui/flex";
import { useActorsView } from "../actors-view-context-provider";
import { useEngineCompatDataProvider } from "../data-provider";
import * as ActorCreateForm from "../form/actor-create-form";

export default function CreateActorDialog() {
	const dataProvider = useEngineCompatDataProvider();
	const navigate = useNavigate();
	const { mutateAsync } = useMutation({
		...dataProvider.createActorMutationOptions(),
		onSuccess: async (data) => {
			const stringKeys = dataProvider
				.actorsQueryOptions({})
				.queryKey.filter((k): k is string => typeof k === "string");
			await queryClient.invalidateQueries({
				predicate: (query) =>
					stringKeys.every((k) => query.queryKey.includes(k)),
			});
			return navigate({
				to: ".",
				search: (old) => ({ ...old, modal: undefined, actorId: data }),
			});
		},
	});
	const name = useSearch({
		from: "/_context",
		select: (state) => state.n?.[0],
	});

	const { copy } = useActorsView();
	const defaultRunnerNameSelector =
		ActorCreateForm.useDefaultRunnerNameSelector();

	return (
		<ActorCreateForm.Form
			onSubmit={async (values) => {
				await mutateAsync({
					name: values.name,
					input: values.input ? JSON.parse(values.input) : undefined,
					key: values.key,
					datacenter: values.datacenter,
					runnerNameSelector:
						values.runnerNameSelector || defaultRunnerNameSelector,
					crashPolicy: "destroy",
				});
			}}
			defaultValues={{
				name,
				key: getRandomKey(),
			}}
		>
			<DialogHeader>
				<DialogTitle>{copy.createActorModal.title(name)}</DialogTitle>
				<DialogDescription>
					{copy.createActorModal.description}
				</DialogDescription>
			</DialogHeader>
			<Flex gap="4" direction="col">
				{!name ? <ActorCreateForm.Build /> : null}
				<ActorCreateForm.Keys />
				<ActorCreateForm.ActorPreview />
				{features.datacenter ? (
					<>
						<ActorCreateForm.PrefillActorName />
						<ActorCreateForm.PrefillRunnerName />
						<ActorCreateForm.PrefillDatacenter />
					</>
				) : null}

				<Accordion type="single" collapsible>
					<AccordionItem value="item-1">
						<AccordionTrigger>Advanced</AccordionTrigger>
						<AccordionContent className="flex gap-4 flex-col">
							{features.datacenter ? (
								<>
									<ActorCreateForm.Datacenter />
									<ActorCreateForm.RunnerNameSelector />
								</>
							) : (
								<ActorCreateForm.Pool />
							)}
							<ActorCreateForm.JsonInput />
						</AccordionContent>
					</AccordionItem>
				</Accordion>
			</Flex>
			<DialogFooter>
				<ActorCreateForm.Submit allowPristine type="submit">
					Create
				</ActorCreateForm.Submit>
			</DialogFooter>
		</ActorCreateForm.Form>
	);
}
