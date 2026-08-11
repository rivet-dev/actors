use proc_macro::TokenStream;
use quote::{ToTokens, quote};
use syn::{
	GenericArgument, Ident, Item, ItemFn, ItemStruct, LitStr, PathArguments, ReturnType, Type,
	parse::{Parse, ParseStream},
	parse_macro_input,
	spanned::Spanned,
};

#[derive(Default)]
enum PruneVariant {
	#[default]
	All,
	History,
	None,
}

#[derive(Default)]
struct WorkflowConfig {
	prune: PruneVariant,
}

#[derive(Default)]
struct OperationConfig {
	max_retries: Option<syn::Expr>,
	timeout: Option<syn::Expr>,
}

impl OperationConfig {
	fn max_retries(&self) -> proc_macro2::TokenStream {
		self.max_retries
			.as_ref()
			.map(|e| e.to_token_stream())
			.unwrap_or_else(|| quote! { 5 })
	}

	fn timeout(&self) -> proc_macro2::TokenStream {
		self.timeout
			.as_ref()
			.map(|e| e.to_token_stream())
			.unwrap_or_else(|| quote! { 30 })
	}
}

struct MessageConfig {
	tail_ttl: u64,
}

impl Default for MessageConfig {
	fn default() -> Self {
		MessageConfig { tail_ttl: 90 }
	}
}

#[proc_macro_attribute]
pub fn workflow(attr: TokenStream, item: TokenStream) -> TokenStream {
	let name = parse_macro_input!(attr as OptionalIdent)
		.ident
		.map(|x| x.to_string())
		.unwrap_or_else(|| "Workflow".to_string());
	let item_fn = parse_macro_input!(item as ItemFn);

	let config = match parse_wf_config(&item_fn.attrs) {
		Ok(x) => x,
		Err(err) => return err.into_compile_error().into(),
	};

	let ctx_ty = syn::parse_str("&mut WorkflowCtx").unwrap();
	let TraitFnOutput {
		ctx_ident,
		input_ident,
		input_type,
		output_type,
	} = match parse_trait_fn(&ctx_ty, "Workflow", &item_fn) {
		Ok(x) => x,
		Err(err) => return err,
	};

	let struct_ident = Ident::new(&name, proc_macro2::Span::call_site());
	let fn_name = item_fn.sig.ident.to_string();
	if !fn_name
		.chars()
		.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
	{
		return error(
			item_fn.sig.ident.span(),
			"invalid workflow name, must be [a-z0-9_]",
		);
	}

	let prune_variant = match config.prune {
		PruneVariant::All => quote! { gas::workflow::PruneVariant::All },
		PruneVariant::History => quote! { gas::workflow::PruneVariant::History },
		PruneVariant::None => quote! { gas::workflow::PruneVariant::None },
	};

	let fn_body = item_fn.block;
	let vis = item_fn.vis;

	let expanded = quote! {
		#vis struct #struct_ident;

		impl gas::workflow::WorkflowInput for #input_type {
			type Workflow = #struct_ident;
		}

		#[async_trait::async_trait]
		impl gas::workflow::Workflow for #struct_ident {
			type Input = #input_type;
			type Output = #output_type;

			const NAME: &'static str = #fn_name;
			const PRUNE_VARIANT: gas::workflow::PruneVariant = #prune_variant;

			async fn run(#ctx_ident: #ctx_ty, #input_ident: &Self::Input) -> Result<Self::Output> {
				#fn_body
			}
		}
	};

	// eprintln!("\n\n{expanded}\n");

	TokenStream::from(expanded)
}

#[proc_macro_attribute]
pub fn activity(attr: TokenStream, item: TokenStream) -> TokenStream {
	let name = parse_macro_input!(attr as Ident).to_string();
	let item_fn = parse_macro_input!(item as ItemFn);

	let config = match parse_op_config(&item_fn.attrs) {
		Ok(x) => x,
		Err(err) => return err.into_compile_error().into(),
	};

	let ctx_ty = syn::parse_str("&ActivityCtx").unwrap();
	let TraitFnOutput {
		ctx_ident,
		input_ident,
		input_type,
		output_type,
	} = match parse_trait_fn(&ctx_ty, "Activity", &item_fn) {
		Ok(x) => x,
		Err(err) => return err,
	};

	let struct_ident = Ident::new(&name, proc_macro2::Span::call_site());
	let fn_name = item_fn.sig.ident.to_string();
	if !fn_name
		.chars()
		.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
	{
		return error(
			item_fn.sig.ident.span(),
			"invalid activity name, must be [a-z0-9_]",
		);
	}

	let fn_body = item_fn.block;
	let vis = item_fn.vis;

	let max_retries = config.max_retries();
	let timeout = config.timeout();

	let expanded = quote! {
		#vis struct #struct_ident;

		impl gas::activity::ActivityInput for #input_type {
			type Activity = #struct_ident;
		}

		#[async_trait::async_trait]
		impl gas::activity::Activity for #struct_ident {
			type Input = #input_type;
			type Output = #output_type;

			const NAME: &'static str = #fn_name;
			const MAX_RETRIES: usize = #max_retries;
			const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(#timeout);

			async fn run(#ctx_ident: #ctx_ty, #input_ident: &Self::Input) -> Result<Self::Output> {
				#fn_body
			}
		}
	};

	// eprintln!("\n\n{expanded}\n");

	TokenStream::from(expanded)
}

#[proc_macro_attribute]
pub fn operation(attr: TokenStream, item: TokenStream) -> TokenStream {
	let name = parse_macro_input!(attr as OptionalIdent)
		.ident
		.map(|x| x.to_string())
		.unwrap_or_else(|| "Operation".to_string());

	let item_fn = parse_macro_input!(item as ItemFn);

	let config = match parse_op_config(&item_fn.attrs) {
		Ok(x) => x,
		Err(err) => return err.into_compile_error().into(),
	};

	let ctx_ty = syn::parse_str("&OperationCtx").unwrap();
	let TraitFnOutput {
		ctx_ident,
		input_ident,
		input_type,
		output_type,
	} = match parse_trait_fn(&ctx_ty, "Operation", &item_fn) {
		Ok(x) => x,
		Err(err) => return err,
	};

	let struct_ident = Ident::new(&name, proc_macro2::Span::call_site());
	let fn_name = item_fn.sig.ident.to_string();
	if !fn_name
		.chars()
		.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
	{
		return error(
			item_fn.sig.ident.span(),
			"invalid operation name, must be [a-z0-9_]",
		);
	}

	let generics = &item_fn.sig.generics;
	let fn_body = item_fn.block;
	let vis = item_fn.vis;

	let timeout = config.timeout();

	let expanded = quote! {
		#vis struct #struct_ident;

		impl gas::operation::OperationInput for #input_type {
			type Operation = #struct_ident;
		}

		#[async_trait::async_trait]
		impl gas::operation::Operation for #struct_ident {
			type Input = #input_type;
			type Output = #output_type;

			const NAME: &'static str = #fn_name;
			const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(#timeout);

			async fn run #generics (#ctx_ident: #ctx_ty, #input_ident: &Self::Input) -> Result<Self::Output> {
				#fn_body
			}
		}
	};

	// eprintln!("\n\n{expanded}\n");

	TokenStream::from(expanded)
}

struct TraitFnOutput {
	ctx_ident: syn::Ident,
	input_ident: syn::Ident,
	input_type: Box<syn::Type>,
	output_type: syn::Type,
}

fn parse_trait_fn(
	ctx_ty: &syn::Type,
	trait_name: &str,
	item_fn: &syn::ItemFn,
) -> Result<TraitFnOutput, TokenStream> {
	// Check if is async
	if item_fn.sig.asyncness.is_none() {
		return Err(error(item_fn.sig.span(), "function must be async"));
	}

	let mut arg_names = vec![];
	let mut arg_types = vec![];

	for input in &item_fn.sig.inputs {
		if let syn::FnArg::Typed(arg) = input {
			match arg.pat.as_ref() {
				syn::Pat::Ident(ident) => {
					let arg_name = ident.ident.to_string();
					arg_names.push(arg_name);
					arg_types.push((*arg.ty).clone());
				}
				_ => {
					return Err(error(arg.pat.span(), "unsupported input parameter pattern"));
				}
			}
		} else {
			return Err(error(input.span(), "unsupported input parameter type"));
		}
	}

	if arg_types.len() != 2 || &arg_types[0] != ctx_ty {
		return Err(error(
			item_fn.sig.span(),
			&format!(
				"{} function must have exactly two parameters: ctx: {} and input: &YourInputType",
				trait_name,
				ctx_ty.to_token_stream()
			),
		));
	}

	let input_type = if let syn::Type::Reference(syn::TypeReference { elem, .. }) = &arg_types[1] {
		elem.clone()
	} else {
		return Err(error(arg_types[1].span(), "input type must be a reference"));
	};

	let output_type = match &item_fn.sig.output {
		ReturnType::Type(_, ty) => match ty.as_ref() {
			Type::Path(path) => {
				let segment = path.path.segments.last().unwrap();
				if segment.ident == "Result" {
					match &segment.arguments {
						PathArguments::AngleBracketed(args) => {
							if let Some(GenericArgument::Type(ty)) = args.args.first() {
								ty.clone()
							} else {
								return Err(error(args.span(), "unsupported Result type"));
							}
						}
						_ => {
							return Err(error(segment.arguments.span(), "unsupported Result type"));
						}
					}
				} else {
					return Err(error(
						path.span(),
						&format!("{} function must return a Result type", trait_name),
					));
				}
			}
			_ => return Err(error(ty.span(), "unsupported output type")),
		},
		_ => {
			return Err(error(
				item_fn.sig.output.span(),
				&format!("{} function must have a return type", trait_name),
			));
		}
	};

	Ok(TraitFnOutput {
		ctx_ident: Ident::new(&arg_names[0], proc_macro2::Span::call_site()),
		input_ident: Ident::new(&arg_names[1], proc_macro2::Span::call_site()),
		input_type,
		output_type,
	})
}

#[proc_macro_attribute]
pub fn signal(attr: TokenStream, item: TokenStream) -> TokenStream {
	let name = parse_macro_input!(attr as LitStr);
	if !name
		.value()
		.chars()
		.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
	{
		return error(name.span(), "invalid signal name, must be [a-z0-9_]");
	}

	let item = parse_macro_input!(item as Item);
	let (ident, attrs) = match item {
		Item::Struct(ref item_struct) => (&item_struct.ident, &item_struct.attrs),
		Item::Enum(ref item_enum) => (&item_enum.ident, &item_enum.attrs),
		_ => return error(item.span(), "expected struct or enum"),
	};

	// If also a message, don't derive serde traits
	let also_message = attrs
		.iter()
		.filter_map(|attr| attr.path().segments.last())
		.any(|seg| seg.ident == "message");
	let serde_derive = if also_message {
		quote! {}
	} else {
		quote! { #[derive(serde::Serialize, serde::Deserialize)] }
	};

	let expanded = quote! {
		#serde_derive
		#item

		impl gas::signal::Signal for #ident {
			const NAME: &'static str = #name;
		}

		#[async_trait::async_trait]
		impl gas::listen::Listen for #ident {
			async fn listen(ctx: &mut gas::prelude::ListenCtx, limit: usize) -> gas::prelude::WorkflowResult<Vec<Self>> {
				ctx
					.listen_any(&[<Self as gas::signal::Signal>::NAME], limit)
					.await?
					.into_iter()
					.map(|signal| Self::parse(&signal.signal_name, &signal.body))
					.collect()
			}

			fn parse(_name: &str, body: &serde_json::value::RawValue) -> gas::prelude::WorkflowResult<Self> {
				rivet_util::serde::json_from_str!(body.get()).map_err(WorkflowError::DeserializeSignalBody)
			}
		}
	};

	TokenStream::from(expanded)
}

#[proc_macro_attribute]
pub fn message(attr: TokenStream, item: TokenStream) -> TokenStream {
	let name = parse_macro_input!(attr as LitStr);
	if !name
		.value()
		.chars()
		.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
	{
		return error(name.span(), "invalid message name, must be [a-z0-9_]");
	}

	let item_struct = parse_macro_input!(item as ItemStruct);

	// If also a signal, don't derive serde traits
	let also_signal = item_struct
		.attrs
		.iter()
		.filter_map(|attr| attr.path().segments.last())
		.any(|seg| seg.ident == "signal");
	let serde_derive = if also_signal {
		quote! {}
	} else {
		quote! { #[derive(serde::Serialize, serde::Deserialize)] }
	};

	let config = match parse_msg_config(&item_struct.attrs) {
		Ok(x) => x,
		Err(err) => return err.into_compile_error().into(),
	};

	let struct_ident = &item_struct.ident;
	let tail_ttl = config.tail_ttl;

	let expanded = quote! {
		#serde_derive
		#[derive(Debug)]
		#item_struct

		impl gas::message::Message for #struct_ident {
			const NAME: &'static str = #name;
			const TAIL_TTL: std::time::Duration = std::time::Duration::from_secs(#tail_ttl);
		}
	};

	TokenStream::from(expanded)
}

fn error(span: proc_macro2::Span, msg: &str) -> TokenStream {
	syn::Error::new(span, msg).to_compile_error().into()
}

fn parse_wf_config(attrs: &[syn::Attribute]) -> syn::Result<WorkflowConfig> {
	let mut config = WorkflowConfig::default();

	for attr in attrs {
		let syn::Meta::NameValue(name_value) = &attr.meta else {
			continue;
		};

		let ident = name_value.path.require_ident()?;

		// Verify config property
		if ident == "prune" {
			let variant = syn::parse::<syn::Ident>(name_value.value.to_token_stream().into())?;

			config.prune = match variant.to_string().as_str() {
				"all" => PruneVariant::All,
				"history" => PruneVariant::History,
				"none" => PruneVariant::None,
				_ => {
					return Err(syn::Error::new(
						name_value.value.span(),
						format!("Invalid prune variant. Expected: all, history, none"),
					));
				}
			};
		} else if ident != "doc" {
			return Err(syn::Error::new(
				name_value.span(),
				format!("Unknown config property `{ident}`"),
			));
		}
	}

	Ok(config)
}

fn parse_op_config(attrs: &[syn::Attribute]) -> syn::Result<OperationConfig> {
	let mut config = OperationConfig::default();

	for attr in attrs {
		let syn::Meta::NameValue(name_value) = &attr.meta else {
			continue;
		};

		let ident = name_value.path.require_ident()?;

		// Verify config property
		if ident == "max_retries" {
			config.max_retries = Some(syn::parse::<syn::Expr>(
				name_value.value.to_token_stream().into(),
			)?);
		} else if ident == "timeout" {
			config.timeout = Some(syn::parse::<syn::Expr>(
				name_value.value.to_token_stream().into(),
			)?);
		} else if ident != "doc" {
			return Err(syn::Error::new(
				name_value.span(),
				format!("Unknown config property `{ident}`"),
			));
		}
	}

	Ok(config)
}

fn parse_msg_config(attrs: &[syn::Attribute]) -> syn::Result<MessageConfig> {
	let mut config = MessageConfig::default();

	for attr in attrs {
		let syn::Meta::NameValue(name_value) = &attr.meta else {
			continue;
		};

		let ident = name_value.path.require_ident()?;

		// Verify config property
		if ident == "tail_ttl" {
			config.tail_ttl = syn::parse::<syn::LitInt>(name_value.value.to_token_stream().into())?
				.base10_parse()?;
		} else if ident != "doc" {
			return Err(syn::Error::new(
				name_value.span(),
				format!("Unknown config property `{ident}`"),
			));
		}
	}

	Ok(config)
}

#[allow(dead_code)]
fn parse_empty_config(attrs: &[syn::Attribute]) -> syn::Result<()> {
	for attr in attrs {
		let syn::Meta::NameValue(name_value) = &attr.meta else {
			continue;
		};

		let ident = name_value.path.require_ident()?;

		if ident != "doc" {
			return Err(syn::Error::new(
				name_value.span(),
				format!("Unknown config property `{ident}`"),
			));
		}
	}

	Ok(())
}

struct OptionalIdent {
	ident: Option<Ident>,
}

impl Parse for OptionalIdent {
	fn parse(input: ParseStream) -> syn::Result<Self> {
		if input.is_empty() {
			Ok(OptionalIdent { ident: None })
		} else {
			let ident: Ident = input.parse()?;
			Ok(OptionalIdent { ident: Some(ident) })
		}
	}
}
