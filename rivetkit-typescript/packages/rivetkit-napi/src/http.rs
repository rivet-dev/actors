use std::{collections::HashMap, sync::Arc};

use anyhow::Result;
use napi::{
	Env, JsObject,
	bindgen_prelude::{Buffer, Promise},
};
use napi_derive::napi;
use rivetkit_core::{
	ActorContext as CoreActorContext, ActorHttpResponse, HTTP_BODY_STREAM_CHANNEL_CAPACITY,
	HttpRequestBodyStream as CoreHttpRequestBodyStream, Request, Response, ResponseChunk,
	StreamingResponse,
};
use tokio::sync::{Mutex, mpsc};

use crate::{
	actor_context::ActorContext,
	actor_factory::{CallbackTsfn, TsfnPayloadSummary, callback_error, log_tsfn_invocation},
	cancellation_token::CancellationToken,
};

#[napi(object)]
pub struct JsHttpResponse {
	pub status: Option<u16>,
	pub headers: Option<HashMap<String, String>>,
	pub body: Option<Buffer>,
	pub stream: Option<bool>,
}

#[derive(Clone)]
pub(crate) struct HttpRequestPayload {
	pub(crate) ctx: CoreActorContext,
	pub(crate) request: Request,
	pub(crate) cancel_token: Option<tokio_util::sync::CancellationToken>,
	pub(crate) response_stream: Option<HttpResponseBodyStream>,
}

impl TsfnPayloadSummary for HttpRequestPayload {
	fn payload_summary(&self) -> String {
		format!(
			"actor_id={} method={} uri={} headers={} body_bytes={} has_cancel_token={}",
			self.ctx.actor_id(),
			self.request.method(),
			self.request.uri(),
			self.request.headers().len(),
			self.request.body().len(),
			self.cancel_token.is_some()
		)
	}
}

/// JavaScript-facing writer for a streaming actor HTTP response.
#[napi]
#[derive(Clone)]
pub struct HttpResponseBodyStream {
	tx: Arc<Mutex<Option<mpsc::Sender<ResponseChunk>>>>,
	closed_tx: mpsc::Sender<ResponseChunk>,
}

impl HttpResponseBodyStream {
	pub(crate) fn new(tx: mpsc::Sender<ResponseChunk>) -> Self {
		Self {
			tx: Arc::new(Mutex::new(Some(tx.clone()))),
			closed_tx: tx,
		}
	}

	async fn take_sender(&self) -> napi::Result<mpsc::Sender<ResponseChunk>> {
		self.tx
			.lock()
			.await
			.take()
			.ok_or_else(|| napi::Error::from_reason("http response body stream is already closed"))
	}
}

#[napi]
impl HttpResponseBodyStream {
	#[napi]
	pub async fn cancelled(&self) {
		self.closed_tx.closed().await;
	}

	#[napi]
	pub async fn write(&self, chunk: Buffer) -> napi::Result<()> {
		let tx = self.tx.lock().await.clone().ok_or_else(|| {
			napi::Error::from_reason("http response body stream is already closed")
		})?;
		tx.send(ResponseChunk::Data {
			data: chunk.to_vec(),
			finish: false,
		})
		.await
		.map_err(|_| napi::Error::from_reason("http response body stream receiver dropped"))
	}

	#[napi]
	pub async fn end(&self) -> napi::Result<()> {
		let tx = self.take_sender().await?;
		tx.send(ResponseChunk::Data {
			data: Vec::new(),
			finish: true,
		})
		.await
		.map_err(|_| napi::Error::from_reason("http response body stream receiver dropped"))
	}

	#[napi]
	pub async fn error(&self, message: String) -> napi::Result<()> {
		let tx = self.take_sender().await?;
		tx.send(ResponseChunk::Error(message))
			.await
			.map_err(|_| napi::Error::from_reason("http response body stream receiver dropped"))
	}
}

/// JavaScript-facing reader for a streaming actor HTTP request.
#[napi]
pub struct HttpRequestBodyStream {
	// Core can include bytes received with the request-start message. They must be
	// delivered before subsequent chunks from the stream channel.
	initial_body: Arc<Mutex<Option<Vec<u8>>>>,
	rx: Arc<Mutex<Option<CoreHttpRequestBodyStream>>>,
	cancel: tokio_util::sync::CancellationToken,
}

impl HttpRequestBodyStream {
	fn new(initial_body: Vec<u8>, rx: CoreHttpRequestBodyStream) -> Self {
		Self {
			initial_body: Arc::new(Mutex::new(Some(initial_body))),
			rx: Arc::new(Mutex::new(Some(rx))),
			cancel: tokio_util::sync::CancellationToken::new(),
		}
	}
}

#[napi]
impl HttpRequestBodyStream {
	#[napi]
	pub async fn read(&self) -> napi::Result<Option<Buffer>> {
		if self.cancel.is_cancelled() {
			return Ok(None);
		}

		if let Some(initial_body) = self.initial_body.lock().await.take() {
			if self.cancel.is_cancelled() {
				return Ok(None);
			}
			if !initial_body.is_empty() {
				return Ok(Some(Buffer::from(initial_body)));
			}
		}

		let mut rx = self.rx.lock().await;
		let Some(rx) = rx.as_mut() else {
			return Ok(None);
		};
		tokio::select! {
			biased;
			_ = self.cancel.cancelled() => Ok(None),
			chunk = rx.recv() => match chunk {
				Ok(Some(chunk)) => Ok(Some(Buffer::from(chunk))),
				Ok(None) => Ok(None),
				Err(error) => Err(napi::Error::from_reason(format!(
					"streaming request body aborted: {error}"
				))),
			},
		}
	}

	#[napi]
	pub async fn cancel(&self) -> napi::Result<()> {
		self.cancel.cancel();
		self.initial_body.lock().await.take();
		self.rx.lock().await.take();
		Ok(())
	}
}

pub(crate) async fn call_request(
	callback_name: &str,
	callback: &CallbackTsfn<HttpRequestPayload>,
	mut payload: HttpRequestPayload,
) -> Result<ActorHttpResponse> {
	// Expose the writer before invoking JavaScript so the handler can begin
	// producing data before its response promise resolves.
	let (body_tx, body_rx) = mpsc::channel(HTTP_BODY_STREAM_CHANNEL_CAPACITY);
	payload.response_stream = Some(HttpResponseBodyStream::new(body_tx));

	log_tsfn_invocation(callback_name, &payload);
	let promise = callback
		.call_async::<Promise<JsHttpResponse>>(Ok(payload))
		.await
		.map_err(|error| callback_error(callback_name, error))?;
	let response = promise
		.await
		.map_err(|error| callback_error(callback_name, error))?;

	let status = response.status.unwrap_or(200);
	let headers = response.headers.unwrap_or_default();
	if response.stream.unwrap_or(false) {
		StreamingResponse::from_parts(status, headers, body_rx).map(ActorHttpResponse::Stream)
	} else {
		Response::from_parts(
			status,
			headers,
			response
				.body
				.unwrap_or_else(|| Buffer::from(Vec::new()))
				.to_vec(),
		)
		.map(ActorHttpResponse::Buffered)
	}
}

pub(crate) fn build_http_request_payload(
	env: &Env,
	payload: HttpRequestPayload,
) -> napi::Result<Vec<napi::JsUnknown>> {
	let mut object = env.create_object()?;
	object.set("ctx", ActorContext::new(payload.ctx))?;
	object.set("request", build_request_object(env, payload.request)?)?;
	match payload.response_stream {
		Some(response_stream) => object.set("responseBodyStream", response_stream)?,
		None => object.set("responseBodyStream", env.get_undefined()?)?,
	}
	match payload.cancel_token {
		Some(cancel_token) => object.set("cancelToken", CancellationToken::new(cancel_token))?,
		None => object.set("cancelToken", env.get_undefined()?)?,
	}
	Ok(vec![object.into_unknown()])
}

pub(crate) fn build_request_object(env: &Env, request: Request) -> napi::Result<JsObject> {
	let (method, uri, headers, body) = request.to_parts();
	let body_stream = request.take_body_stream();
	let mut request_object = env.create_object()?;
	request_object.set("method", method)?;
	request_object.set("uri", uri)?;
	request_object.set("headers", headers)?;
	if let Some(body_stream) = body_stream {
		request_object.set("body", env.get_undefined()?)?;
		request_object.set("bodyStream", HttpRequestBodyStream::new(body, body_stream))?;
	} else {
		request_object.set("body", Buffer::from(body))?;
		request_object.set("bodyStream", env.get_undefined()?)?;
	}
	Ok(request_object)
}

// Test shim keeps moved tests in crate-root tests/ with private-module access.
#[cfg(test)]
#[path = "../tests/http.rs"]
mod tests;
