import type { Story } from "@ladle/react";
import { faTable, faTableCells, Icon } from "@rivet-gg/icons";
import "../../../../.ladle/ladle.css";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "../../ui/select";
import type { DatabaseColumn } from "../actor-inspector-context";
import { DatabaseTable } from "./database-table";

const columns: DatabaseColumn[] = [
	{
		cid: 0,
		name: "id",
		type: "INTEGER",
		notnull: true,
		dflt_value: null,
		pk: true,
	},
	{
		cid: 1,
		name: "has_initialized",
		type: "INTEGER",
		notnull: true,
		dflt_value: null,
		pk: false,
	},
	{
		cid: 2,
		name: "input",
		type: "BLOB",
		notnull: false,
		dflt_value: null,
		pk: false,
	},
];

export const SchemaTypesAndBlob: Story = () => (
	<div className="min-h-screen bg-background p-8 text-foreground">
		<div className="w-fit max-w-full overflow-hidden rounded-md border">
			<div className="flex h-[45px] items-center border-b">
				<div className="h-full border-r">
					<Select defaultValue="_rivet_actor">
						<SelectTrigger
							variant="ghost"
							className="h-full rounded-none pr-2 [&>svg]:!size-3"
						>
							<SelectValue />
						</SelectTrigger>
						<SelectContent>
							<SelectItem value="_rivet_actor">
								<span className="flex items-center gap-2">
									<Icon icon={faTable} />
									_rivet_actor
								</span>
							</SelectItem>
						</SelectContent>
					</Select>
				</div>
				<div className="flex items-center gap-2 px-3 text-sm">
					<Icon icon={faTableCells} className="size-3" />
					main._rivet_actor
					<span className="text-muted-foreground">
						(3 columns, 1 row)
					</span>
				</div>
			</div>
			<DatabaseTable
				columns={columns}
				data={[{ id: 1, has_initialized: 1, input: new Uint8Array() }]}
				enableColumnResizing={false}
				enableRowSelection={false}
			/>
		</div>
	</div>
);
