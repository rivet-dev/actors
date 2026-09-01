#if !UNITY_WEBGL || UNITY_EDITOR

using System;
using System.Collections;
using System.Collections.Generic;
using System.IO;
using System.Net;
using System.Text;
using Unity.WebRTC;
using UnityEngine;

namespace cakeslice.SimpleWebRTC
{
	public sealed class SimpleWebRTCClientNative : SimpleWebRTCClient
	{
		[Serializable]
		private struct SignalingMessage
		{
			public ushort connId;
			public string sdp;
			public string[] candidates;
		}

		private readonly object sync = new object();
		private readonly List<string> localCandidates = new List<string>();
		private readonly Queue<Tuple<byte[], Common.DeliveryMethod>> connectingSendQueue =
			new Queue<Tuple<byte[], Common.DeliveryMethod>>();

		private RTCPeerConnection peer;
		private RTCDataChannel reliableChannel;
		private RTCDataChannel unreliableChannel;
		private GameObject signalingHost;
		private bool connectedEventQueued;
		private bool disconnectEventQueued;

		internal SimpleWebRTCClientNative(int maxMessageSize, int maxMessagesPerTick)
			: base(maxMessageSize, maxMessagesPerTick)
		{
		}

		public override void Connect(List<Common.ICEServer> iceServers, Uri serverAddress)
		{
			if (state != ClientState.NotConnected)
				return;

			state = ClientState.Connecting;
			signalingHost = new GameObject("FishyWebRTC Native Signaling");
			signalingHost.hideFlags = HideFlags.HideAndDontSave;
			UnityEngine.Object.DontDestroyOnLoad(signalingHost);
			signalingHost.AddComponent<SimpleWebRTCNativeCoroutineHost>()
				.StartCoroutine(ConnectCoroutine(iceServers, serverAddress));
		}

		private IEnumerator ConnectCoroutine(List<Common.ICEServer> iceServers, Uri serverAddress)
		{
				peer = new RTCPeerConnection();
				var rtcIceServers = new List<RTCIceServer>();
				foreach (Common.ICEServer server in iceServers)
				{
					var rtcServer = new RTCIceServer
					{
						urls = new[] { server.url },
					};
					if (!string.IsNullOrEmpty(server.username))
					{
						rtcServer.username = server.username;
						rtcServer.credential = server.credential;
						rtcServer.credentialType = RTCIceCredentialType.Password;
					}
					rtcIceServers.Add(rtcServer);
				}

				var configuration = new RTCConfiguration
				{
					iceServers = rtcIceServers.ToArray(),
				};
				RTCErrorType configurationError = peer.SetConfiguration(ref configuration);
				if (configurationError != RTCErrorType.None)
					throw new InvalidOperationException($"SetConfiguration failed: {configurationError}");

				peer.OnIceCandidate = candidate =>
				{
					if (candidate == null || string.IsNullOrEmpty(candidate.Candidate))
						return;
					if (candidate.Protocol != RTCIceProtocol.Udp)
						return;
					lock (sync)
						localCandidates.Add(candidate.Candidate);
					Debug.Log(
						$"[webrtc] client ICE candidate: type={candidate.Type} " +
						$"protocol={candidate.Protocol} address={candidate.Address}:{candidate.Port} " +
						$"related={candidate.RelatedAddress}:{candidate.RelatedPort}");
				};
				peer.OnIceGatheringStateChange = gatheringState =>
					Debug.Log($"[webrtc] client ICE gathering: {gatheringState}");
				peer.OnIceConnectionChange = iceState =>
					Debug.Log($"[webrtc] client ICE connection: {iceState}");
				peer.OnDataChannel = ConfigureChannel;
				peer.OnConnectionStateChange = connectionState =>
				{
					Debug.Log($"[webrtc] client peer connection: {connectionState}");
					if (connectionState == RTCPeerConnectionState.Connected)
						TryQueueConnected();
					else if (
						connectionState == RTCPeerConnectionState.Closed ||
						connectionState == RTCPeerConnectionState.Disconnected ||
						connectionState == RTCPeerConnectionState.Failed)
						QueueDisconnected();
				};

				Debug.Log("[webrtc] client signaling: requesting offer");
				var offer = JsonUtility.FromJson<SignalingMessage>(
					SendHttp(serverAddress, "offer/", "GET", null));
				Debug.Log("[webrtc] client signaling: received offer");
				var offerDescription = new RTCSessionDescription
				{
					type = RTCSdpType.Offer,
					sdp = offer.sdp,
				};
				var remoteOffer = peer.SetRemoteDescription(ref offerDescription);
				yield return remoteOffer;
				if (remoteOffer.IsError)
				{
					FailConnection(new InvalidOperationException($"SetRemoteDescription failed: {remoteOffer.Error.message}"));
					yield break;
				}
				Debug.Log("[webrtc] client signaling: applied remote offer");

				var answerOperation = peer.CreateAnswer();
				yield return answerOperation;
				if (answerOperation.IsError)
				{
					FailConnection(new InvalidOperationException($"CreateAnswer failed: {answerOperation.Error.message}"));
					yield break;
				}
				Debug.Log("[webrtc] client signaling: created answer");

				RTCSessionDescription answerDescription = answerOperation.Desc;
				var localAnswer = peer.SetLocalDescription(ref answerDescription);
				yield return localAnswer;
				if (localAnswer.IsError)
				{
					FailConnection(new InvalidOperationException($"SetLocalDescription failed: {localAnswer.Error.message}"));
					yield break;
				}
				Debug.Log("[webrtc] client signaling: applied local answer");

				// A host candidate usually arrives first, but it cannot reach a server
				// across the public internet. Wait for ICE gathering to finish so the
				// STUN-reflexive (or TURN relay) candidate is included as well.
				float gatheringDeadline = Time.realtimeSinceStartup + 10f;
				while (
					peer.GatheringState != RTCIceGatheringState.Complete &&
					Time.realtimeSinceStartup < gatheringDeadline)
				{
					yield return null;
				}

				string[] candidates;
				lock (sync)
					candidates = localCandidates.ToArray();
				if (candidates.Length == 0)
					throw new InvalidOperationException("ICE gathering completed without any local candidates");
				var answer = new SignalingMessage
				{
					connId = offer.connId,
					sdp = answerDescription.sdp,
					candidates = candidates,
				};
				Debug.Log($"[webrtc] client signaling: posting answer with {candidates.Length} candidate(s)");
				var response = JsonUtility.FromJson<SignalingMessage>(
					SendHttp(serverAddress, "answer/", "POST", JsonUtility.ToJson(answer)));
				Debug.Log($"[webrtc] client signaling: received {response.candidates?.Length ?? 0} server candidate(s)");

				if (response.candidates != null)
				{
					foreach (string candidate in response.candidates)
					{
						if (string.IsNullOrEmpty(candidate))
							continue;
						var candidateInit = new RTCIceCandidateInit
						{
							candidate = candidate,
							sdpMid = "0",
							sdpMLineIndex = 0,
						};
						peer.AddIceCandidate(new RTCIceCandidate(candidateInit));
					}
				}

				yield return LogSelectedIcePair();
		}

		private IEnumerator LogSelectedIcePair()
		{
			float connectionDeadline = Time.realtimeSinceStartup + 30f;
			while (
				peer != null &&
				peer.ConnectionState != RTCPeerConnectionState.Connected &&
				Time.realtimeSinceStartup < connectionDeadline)
			{
				yield return null;
			}

			if (peer == null || peer.ConnectionState != RTCPeerConnectionState.Connected)
			{
				Debug.LogError("[webrtc] ICE PROOF FAILED: peer did not connect");
				yield break;
			}

			float statsDeadline = Time.realtimeSinceStartup + 10f;
			while (peer != null && Time.realtimeSinceStartup < statsDeadline)
			{
				RTCStatsReportAsyncOperation operation = peer.GetStats();
				yield return operation;
				if (operation.IsError || operation.Value == null)
				{
					Debug.LogError("[webrtc] ICE PROOF FAILED: GetStats did not return a report");
					yield break;
				}

				using (RTCStatsReport report = operation.Value)
				{
					foreach (RTCStats stats in report.Stats.Values)
					{
						var transport = stats as RTCTransportStats;
						if (transport == null || string.IsNullOrEmpty(transport.selectedCandidatePairId))
							continue;

						if (!report.TryGetValue(
								transport.selectedCandidatePairId,
								out RTCStats selectedPairStats))
						{
							continue;
						}

						var pair = selectedPairStats as RTCIceCandidatePairStats;
						if (pair == null || !pair.nominated || pair.state != "succeeded")
							continue;
						if (pair.bytesSent == 0 || pair.bytesReceived == 0)
							continue;

						if (!report.TryGetValue(pair.localCandidateId, out RTCStats localStats) ||
							!report.TryGetValue(pair.remoteCandidateId, out RTCStats remoteStats))
						{
							continue;
						}

						var local = localStats as RTCIceCandidateStats;
						var remote = remoteStats as RTCIceCandidateStats;
						if (local == null || remote == null)
							continue;

						bool relayed =
							string.Equals(local.candidateType, "relay", StringComparison.OrdinalIgnoreCase) ||
							string.Equals(remote.candidateType, "relay", StringComparison.OrdinalIgnoreCase);
						bool direct = IsDirectUdpCandidate(local) && IsDirectUdpCandidate(remote);
						string verdict = relayed
							? "TURN RELAY"
							: direct
								? "DIRECT P2P (NO TURN)"
								: "UNVERIFIED ICE PATH";
						Debug.Log(
							$"[webrtc] SELECTED ICE PAIR: " +
							$"id={transport.selectedCandidatePairId} " +
							$"local={local.candidateType}/{local.protocol} " +
							$"remote={remote.candidateType}/{remote.protocol} " +
							$"nominated={pair.nominated} state={pair.state} " +
							$"bytesSent={pair.bytesSent} bytesReceived={pair.bytesReceived} => {verdict}");
						if (direct)
							Debug.Log("[webrtc] ICE PROOF PASSED: native UDP P2P with no TURN relay");
						else
							Debug.LogError(
								relayed
									? "[webrtc] ICE PROOF FAILED: selected pair contains a relay candidate"
									: "[webrtc] ICE PROOF FAILED: selected pair is not a recognized direct UDP path");
						yield break;
					}
				}

				yield return new WaitForSecondsRealtime(0.25f);
			}

			Debug.LogError("[webrtc] ICE PROOF FAILED: no nominated succeeded candidate pair in stats");
		}

		private static bool IsDirectUdpCandidate(RTCIceCandidateStats candidate)
		{
			if (!string.Equals(candidate.protocol, "udp", StringComparison.OrdinalIgnoreCase))
				return false;

			return
				string.Equals(candidate.candidateType, "host", StringComparison.OrdinalIgnoreCase) ||
				string.Equals(candidate.candidateType, "srflx", StringComparison.OrdinalIgnoreCase) ||
				string.Equals(candidate.candidateType, "prflx", StringComparison.OrdinalIgnoreCase);
		}

		private void FailConnection(Exception exception)
		{
			Log.Exception(exception);
			receiveQueue.Enqueue(new Message(exception));
			QueueDisconnected();
		}

		private void ConfigureChannel(RTCDataChannel channel)
		{
			if (channel.Label == "Reliable")
				reliableChannel = channel;
			else if (channel.Label == "Unreliable")
				unreliableChannel = channel;
			else
				return;

			channel.OnOpen = TryQueueConnected;
			channel.OnClose = QueueDisconnected;
			channel.OnMessage = bytes =>
			{
				if (bytes.Length > maxMessageSize)
				return;
				ArrayBuffer buffer = bufferPool.Take(bytes.Length);
				buffer.CopyFrom(bytes, 0, bytes.Length);
				receiveQueue.Enqueue(new Message(buffer));
			};
			TryQueueConnected();
		}

		private void TryQueueConnected()
		{
			lock (sync)
			{
				if (
					connectedEventQueued ||
					reliableChannel == null ||
					unreliableChannel == null ||
					reliableChannel.ReadyState != RTCDataChannelState.Open ||
					unreliableChannel.ReadyState != RTCDataChannelState.Open)
					return;

				connectedEventQueued = true;
				state = ClientState.Connected;
				receiveQueue.Enqueue(new Message(Common.EventType.Connected));

				while (connectingSendQueue.Count > 0)
				{
					Tuple<byte[], Common.DeliveryMethod> queued = connectingSendQueue.Dequeue();
					SendBytes(queued.Item1, queued.Item2);
				}
			}
		}

		public override void Send(ArraySegment<byte> segment, Common.DeliveryMethod deliveryMethod)
		{
			if (segment.Count > maxMessageSize)
			{
				Log.Error($"Cant send message with length {segment.Count} because it is over the max size of {maxMessageSize}");
				return;
			}

			byte[] bytes = new byte[segment.Count];
			Array.Copy(segment.Array, segment.Offset, bytes, 0, segment.Count);
			lock (sync)
			{
				if (state == ClientState.Connected)
					SendBytes(bytes, deliveryMethod);
				else if (state == ClientState.Connecting)
					connectingSendQueue.Enqueue(Tuple.Create(bytes, deliveryMethod));
			}
		}

		private void SendBytes(byte[] bytes, Common.DeliveryMethod deliveryMethod)
		{
			RTCDataChannel channel = deliveryMethod == Common.DeliveryMethod.Unreliable
				? unreliableChannel
				: reliableChannel;
			if (channel != null && channel.ReadyState == RTCDataChannelState.Open)
				channel.Send(bytes);
		}

		public override void Disconnect()
		{
			if (state == ClientState.NotConnected)
				return;

			state = ClientState.Disconnecting;
			if (signalingHost != null)
			{
				UnityEngine.Object.Destroy(signalingHost);
				signalingHost = null;
			}
			reliableChannel?.Dispose();
			reliableChannel = null;
			unreliableChannel?.Dispose();
			unreliableChannel = null;
			peer?.Dispose();
			peer = null;
			QueueDisconnected();
		}

		private void QueueDisconnected()
		{
			lock (sync)
			{
				if (disconnectEventQueued)
					return;
				disconnectEventQueued = true;
				state = ClientState.NotConnected;
				receiveQueue.Enqueue(new Message(Common.EventType.Disconnected));
			}
		}

		private static string SendHttp(Uri baseAddress, string relativePath, string method, string body)
		{
			var request = (HttpWebRequest)WebRequest.Create(new Uri(baseAddress, relativePath));
			request.Method = method;
			request.Accept = "application/json";
			request.ContentType = "application/json";
			// The server waits for ICE gathering (up to 10 seconds) before returning
			// its candidates, so the signaling request must have a wider deadline.
			request.Timeout = 30000;

			if (body != null)
			{
				byte[] bodyBytes = Encoding.UTF8.GetBytes(body);
				request.ContentLength = bodyBytes.Length;
				using Stream requestStream = request.GetRequestStream();
				requestStream.Write(bodyBytes, 0, bodyBytes.Length);
			}

			using var response = (HttpWebResponse)request.GetResponse();
			using var reader = new StreamReader(response.GetResponseStream());
			return reader.ReadToEnd();
		}
	}

	internal sealed class SimpleWebRTCNativeCoroutineHost : MonoBehaviour
	{
	}
}

#endif
