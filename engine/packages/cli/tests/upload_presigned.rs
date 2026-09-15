use std::{
	io::{Read, Write},
	net::{TcpListener, TcpStream},
	thread,
};

use clap::Parser;
use serde_json::json;

use super::*;

fn append(tar: &mut tar::Builder<GzEncoder<File>>, name: &str, body: &[u8]) {
	let mut header = tar::Header::new_gnu();
	header.set_size(body.len() as u64);
	header.set_mode(0o600);
	header.set_cksum();
	tar.append_data(&mut header, name, body).unwrap();
}

fn fixture(path: &Path, architecture: &str, os: &str, images: usize, corrupt_layer: bool) {
	let layer = b"layer contents";
	let config = serde_json::to_vec(&json!({"os":os,"architecture":architecture,"rootfs":{"diff_ids":[format!("sha256:{}",hex::encode(Sha256::digest(layer)))]}})).unwrap();
	let manifest = json!({"Config":"config.json","Layers":["layer.tar"]});
	let encoder = GzEncoder::new(File::create(path).unwrap(), Compression::fast());
	let mut tar = tar::Builder::new(encoder);
	// Deliberately put config before manifest, like real Docker save output.
	append(&mut tar, "config.json", &config);
	append(
		&mut tar,
		"layer.tar",
		if corrupt_layer { b"wrong layer" } else { layer },
	);
	append(
		&mut tar,
		"manifest.json",
		&serde_json::to_vec(&vec![manifest; images]).unwrap(),
	);
	tar.into_inner().unwrap().finish().unwrap();
}

fn read_request(stream: &mut TcpStream) -> (String, Vec<u8>) {
	stream
		.set_read_timeout(Some(Duration::from_secs(10)))
		.unwrap();
	let mut bytes = Vec::new();
	loop {
		let mut byte = [0];
		stream.read_exact(&mut byte).unwrap();
		bytes.push(byte[0]);
		if bytes.ends_with(b"\r\n\r\n") {
			break;
		}
		assert!(bytes.len() < 16384);
	}
	let headers = String::from_utf8(bytes).unwrap();
	let length = headers
		.lines()
		.find_map(|line| {
			line.to_ascii_lowercase()
				.strip_prefix("content-length:")
				.map(|s| s.trim().parse::<usize>().unwrap())
		})
		.unwrap();
	if headers
		.to_ascii_lowercase()
		.contains("expect: 100-continue")
	{
		stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").unwrap();
	}
	let mut body = vec![0; length];
	stream.read_exact(&mut body).unwrap();
	(headers, body)
}

fn server(statuses: Vec<u16>) -> (String, thread::JoinHandle<Vec<Vec<u8>>>) {
	let listener = TcpListener::bind("127.0.0.1:0").unwrap();
	let url = format!(
		"http://{}/image?secret=DO_NOT_PRINT",
		listener.local_addr().unwrap()
	);
	let handle = thread::spawn(move || {
		let mut bodies = Vec::new();
		for status in statuses {
			let (mut stream, _) = listener.accept().unwrap();
			let (headers, body) = read_request(&mut stream);
			assert!(headers.starts_with("PUT /image?secret=DO_NOT_PRINT HTTP/1.1"));
			assert!(
				headers
					.to_ascii_lowercase()
					.contains("content-type: application/gzip")
			);
			assert!(!headers.to_ascii_lowercase().contains("authorization:"));
			bodies.push(body);
			if status == 0 {
				// Drop the connection after receiving the upload, before any response.
				continue;
			}
			// Both Location and the response body deliberately contain sensitive text.
			write!(stream, "HTTP/1.1 {status} Test\r\nContent-Length: 12\r\nLocation: http://127.0.0.1:1/DO_NOT_PRINT\r\nConnection: close\r\n\r\nDO_NOT_PRINT").unwrap();
		}
		bodies
	});
	(url, handle)
}

#[test]
fn validates_platform_single_image_and_layer_integrity() {
	let temp = tempfile::tempdir().unwrap();
	let file = temp.path().join("worker.tar.gz");
	fixture(&file, "amd64", "linux", 1, false);
	let metadata = inspect_archive(&file).unwrap();
	assert_eq!(metadata.architecture, "amd64");
	assert_eq!(
		metadata.sha256,
		hex::encode(Sha256::digest(fs::read(&file).unwrap()))
	);
	for (arch, os, images, corrupt) in [
		("arm64", "linux", 1, false),
		("amd64", "windows", 1, false),
		("amd64", "linux", 2, false),
		("amd64", "linux", 0, false),
		("amd64", "linux", 1, true),
	] {
		fixture(&file, arch, os, images, corrupt);
		assert!(inspect_archive(&file).is_err());
	}
}

#[test]
fn rejects_truncated_and_non_docker_archives() {
	let temp = tempfile::tempdir().unwrap();
	let file = temp.path().join("worker.tar.gz");
	fixture(&file, "amd64", "linux", 1, false);
	let bytes = fs::read(&file).unwrap();
	fs::write(&file, &bytes[..bytes.len() - 5]).unwrap();
	assert!(inspect_archive(&file).is_err());
	fs::write(&file, "not gzip").unwrap();
	assert!(inspect_archive(&file).is_err());
}

#[test]
fn rejects_unsafe_urls_without_echoing_them() {
	for url in [
		"DO_NOT_PRINT",
		"http://example.com/DO_NOT_PRINT",
		"https://user:DO_NOT_PRINT@example.com/",
		"https://example.com/#DO_NOT_PRINT",
		"https://example.com/\nDO_NOT_PRINT",
	] {
		let error = validate_url(url).unwrap_err();
		assert!(!format!("{error:#}").contains("DO_NOT_PRINT"));
	}
	validate_url("https://example.com/object?signature=secret").unwrap();
	validate_url("http://127.0.0.1:9000/object?signature=secret").unwrap();
}

#[test]
fn cli_requires_exactly_one_input_and_restricts_context() {
	let base = [
		"rivet",
		"byoc",
		"workers",
		"builds",
		"upload-presigned",
		"--presigned-build-url",
		"https://example.com/?secret=DO_NOT_PRINT",
	];
	assert!(crate::Cli::try_parse_from(base).is_err());
	for extra in [
		vec!["--image", "worker:latest"],
		vec!["--archive", "image.tar.gz"],
		vec!["--dockerfile", "Dockerfile", "--context", "."],
	] {
		assert!(crate::Cli::try_parse_from(base.into_iter().chain(extra)).is_ok());
	}
	for extra in [
		vec!["--image", "worker:latest", "--archive", "image.tar.gz"],
		vec!["--archive", "image.tar.gz", "--context", "."],
	] {
		let error = crate::Cli::try_parse_from(base.into_iter().chain(extra.clone()))
			.err()
			.unwrap_or_else(|| panic!("accepted conflicting flags: {extra:?}"));
		assert!(!error.to_string().contains("DO_NOT_PRINT"));
	}
}

#[tokio::test]
async fn archive_upload_sends_exact_bytes_without_cloud_auth() {
	let temp = tempfile::tempdir().unwrap();
	let archive = temp.path().join("worker.tar.gz");
	fixture(&archive, "amd64", "linux", 1, false);
	let expected = fs::read(&archive).unwrap();
	let (url, server) = server(vec![200]);
	Opts {
		presigned_build_url: url,
		archive: Some(archive),
		image: None,
		dockerfile: None,
		context: None,
	}
	.execute()
	.await
	.unwrap();
	assert_eq!(server.join().unwrap(), vec![expected]);
}

#[tokio::test]
async fn transport_retries_identical_bytes_and_never_leaks_errors() {
	let temp = tempfile::tempdir().unwrap();
	let archive = temp.path().join("worker.tar.gz");
	fixture(&archive, "amd64", "linux", 1, false);
	let expected = fs::read(&archive).unwrap();
	let (url, receiver) = server(vec![503, 200]);
	upload(&url, &archive, expected.len() as u64).await.unwrap();
	assert_eq!(
		receiver.join().unwrap(),
		vec![expected.clone(), expected.clone()]
	);
	for status in [403, 307, 400] {
		let (url, receiver) = server(vec![status]);
		let error = upload(&url, &archive, expected.len() as u64)
			.await
			.unwrap_err();
		assert!(!format!("{error:#}").contains("DO_NOT_PRINT"));
		assert!(error.to_string().contains(&status.to_string()));
		assert_eq!(receiver.join().unwrap().len(), 1);
	}
}

#[tokio::test]
async fn transport_replays_after_disconnect_and_bounds_retries() {
	let temp = tempfile::tempdir().unwrap();
	let archive = temp.path().join("worker.tar.gz");
	fixture(&archive, "amd64", "linux", 1, false);
	let bytes = fs::read(&archive).unwrap();
	let (url, receiver) = server(vec![0, 200]);
	upload(&url, &archive, bytes.len() as u64).await.unwrap();
	assert_eq!(receiver.join().unwrap(), vec![bytes.clone(); 2]);
	let (url, receiver) = server(vec![503; 3]);
	let error = upload(&url, &archive, bytes.len() as u64)
		.await
		.unwrap_err();
	assert!(error.to_string().contains("HTTP 503"));
	assert!(!format!("{error:#}").contains("DO_NOT_PRINT"));
	assert_eq!(receiver.join().unwrap(), vec![bytes; 3]);
}

#[tokio::test]
async fn transport_errors_do_not_include_the_presigned_url() {
	let temp = tempfile::tempdir().unwrap();
	let archive = temp.path().join("worker.tar.gz");
	fixture(&archive, "amd64", "linux", 1, false);
	let (url, receiver) = server(vec![0; 3]);
	let error = upload(&url, &archive, fs::metadata(&archive).unwrap().len())
		.await
		.unwrap_err();
	assert!(error.to_string().contains("HTTP request failed"));
	assert!(!format!("{error:#}").contains("DO_NOT_PRINT"));
	assert!(!format!("{error:?}").contains(&url));
	assert_eq!(receiver.join().unwrap().len(), 3);
}

#[tokio::test]
async fn dropping_upload_cancels_the_inflight_request() {
	let temp = tempfile::tempdir().unwrap();
	let archive = temp.path().join("worker.tar.gz");
	fixture(&archive, "amd64", "linux", 1, false);
	let size = fs::metadata(&archive).unwrap().len();
	let listener = TcpListener::bind("127.0.0.1:0").unwrap();
	let url = format!(
		"http://{}/upload?secret=DO_NOT_PRINT",
		listener.local_addr().unwrap()
	);
	let (sent, received) = tokio::sync::oneshot::channel();
	let receiver = thread::spawn(move || {
		let (mut stream, _) = listener.accept().unwrap();
		read_request(&mut stream);
		sent.send(()).unwrap();
		// No response: cancellation should close the connection, not wait for timeout.
		assert_eq!(stream.read(&mut [0]).unwrap(), 0);
	});
	let task = tokio::spawn(async move { upload(&url, &archive, size).await });
	tokio::time::timeout(Duration::from_secs(10), received)
		.await
		.unwrap()
		.unwrap();
	task.abort();
	assert!(task.await.unwrap_err().is_cancelled());
	receiver.join().unwrap();
}

#[test]
fn upload_progress_is_readable_ascii_with_speed_and_eta() {
	let mib = 1024 * 1024;
	assert_eq!(
		upload_progress(250 * mib, 500 * mib, 24 * mib, Duration::from_secs(2)),
		"Uploading 50% - 250/500 MiB - 12 MiB/s - 21s remaining"
	);
	assert_eq!(
		upload_progress(250 * mib, 500 * mib, 0, Duration::from_secs(2)),
		"Uploading 50% - 250/500 MiB - 0 MiB/s - ETA unknown"
	);
	assert_eq!(
		upload_progress(mib / 2, mib, mib / 4, Duration::from_secs(2)),
		"Uploading 50% - 0.5/1 MiB - 0.1 MiB/s - 4s remaining"
	);
	assert_eq!(
		upload_progress(500 * mib, 500 * mib, 24 * mib, Duration::from_secs(2)),
		"Uploading 100% - 500/500 MiB - waiting for confirmation"
	);
	assert_eq!(
		upload_progress(0, 500 * mib, 0, Duration::ZERO),
		"Uploading 0% - 0/500 MiB - 0 MiB/s - ETA unknown"
	);
}

#[tokio::test]
async fn upload_reports_progress_every_two_seconds_until_server_response() {
	let temp = tempfile::tempdir().unwrap();
	let archive = temp.path().join("worker.tar.gz");
	fixture(&archive, "amd64", "linux", 1, false);
	let expected = fs::read(&archive).unwrap();
	let size = expected.len() as u64;
	let listener = TcpListener::bind("127.0.0.1:0").unwrap();
	let url = format!(
		"http://{}/upload?secret=DO_NOT_PRINT",
		listener.local_addr().unwrap()
	);
	let (sent, received) = tokio::sync::oneshot::channel();
	let (respond, response) = std::sync::mpsc::channel();
	let receiver = thread::spawn(move || {
		let (mut stream, _) = listener.accept().unwrap();
		assert_eq!(read_request(&mut stream).1, expected);
		sent.send(()).unwrap();
		response.recv_timeout(Duration::from_secs(10)).unwrap();
		stream
			.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
			.unwrap();
	});
	let logs = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
	let writer = logs.clone();
	let task = tokio::spawn(async move {
		upload_with_progress(&url, &archive, size, |line| {
			writer.lock().unwrap().push(line.to_owned())
		})
		.await
	});
	tokio::time::timeout(Duration::from_secs(10), received)
		.await
		.unwrap()
		.unwrap();
	let snapshot = || logs.lock().unwrap().clone();
	assert!(snapshot().is_empty());
	tokio::time::sleep(Duration::from_millis(4300)).await;
	let pending = snapshot();
	assert_eq!(pending.len(), 2, "{pending:?}");
	for line in &pending {
		assert!(line.starts_with("Uploading 100% - "));
		assert!(line.ends_with(" - waiting for confirmation"));
		assert!(line.is_ascii());
		assert!(!line.contains("DO_NOT_PRINT"));
		assert!(!line.contains('\r'));
	}
	respond.send(()).unwrap();
	task.await.unwrap().unwrap();
	receiver.join().unwrap();
	let completed = snapshot();
	tokio::time::sleep(Duration::from_millis(2100)).await;
	assert_eq!(snapshot(), completed, "progress must stop after completion");
}

#[tokio::test]
#[ignore = "requires a running Docker daemon; builds a small local scratch image"]
async fn dockerfile_and_local_image_inputs_upload_amd64() {
	let temp = tempfile::tempdir().unwrap();
	let dockerfile = temp.path().join("Dockerfile");
	fs::write(
		&dockerfile,
		"FROM scratch\nCOPY payload /payload\nCMD [\"/payload\"]\n",
	)
	.unwrap();
	fs::write(temp.path().join("payload"), "worker upload test").unwrap();
	let (url, receiver) = server(vec![200]);
	Opts {
		presigned_build_url: url,
		archive: None,
		image: None,
		dockerfile: Some(dockerfile.clone()),
		context: Some(temp.path().to_owned()),
	}
	.execute()
	.await
	.unwrap();
	let archive = temp.path().join("uploaded.tar.gz");
	fs::write(&archive, &receiver.join().unwrap()[0]).unwrap();
	let image = inspect_archive(&archive).unwrap().image_id;
	let (url, receiver) = server(vec![200]);
	Opts {
		presigned_build_url: url,
		archive: None,
		image: Some(image),
		dockerfile: None,
		context: None,
	}
	.execute()
	.await
	.unwrap();
	fs::write(&archive, &receiver.join().unwrap()[0]).unwrap();
	inspect_archive(&archive).unwrap();
	// A real ARM image must fail before contacting the supplied endpoint.
	let iid = temp.path().join("arm-id");
	docker(&[
		"build",
		"--platform",
		"linux/arm64",
		"--iidfile",
		path(&iid).unwrap(),
		"-f",
		path(&dockerfile).unwrap(),
		path(temp.path()).unwrap(),
	])
	.await
	.unwrap();
	let result = Opts {
		presigned_build_url: "http://127.0.0.1:1/unused".into(),
		archive: None,
		image: Some(fs::read_to_string(iid).unwrap().trim().into()),
		dockerfile: None,
		context: None,
	}
	.execute()
	.await;
	assert!(result.unwrap_err().to_string().contains("linux/amd64"));
}
