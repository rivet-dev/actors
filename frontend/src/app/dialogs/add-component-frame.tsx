import { Frame } from "@/components";
import {
	getProductDocsUrl,
	ProductPicker,
} from "@/components/products/product-picker";

export default function AddComponentFrameContent({
	onClose,
}: {
	onClose?: () => void;
}) {
	return (
		<>
			<Frame.Header>
				<Frame.Title>Add a component</Frame.Title>
				<Frame.Description>
					Pick what you want to add to this project.
				</Frame.Description>
			</Frame.Header>
			<Frame.Content>
				<ProductPicker
					ariaLabel="Add a component"
					onSelect={(target) => {
						window.open(
							getProductDocsUrl(target),
							"_blank",
							"noopener,noreferrer",
						);
						onClose?.();
					}}
				/>
			</Frame.Content>
		</>
	);
}
