using System;
using System.Collections.Generic;
using AOT;

namespace cakeslice.SimpleWebRTC
{
	public class SimpleWebRTCClientWebGL : SimpleWebRTCClient
	{
		static readonly Dictionary<int, SimpleWebRTCClientWebGL> instances = new Dictionary<int, SimpleWebRTCClientWebGL>();

		/// <summary>
		/// key for instances sent between c# and js
		/// </summary>
		int index;

		/// <summary>
		/// Message sent by high level while still connecting, they will be send after onOpen is called
		/// <para>this is a workaround for mirage where send it called right after Connect</para>
		/// </summary>
		Queue<byte[]> ConnectingSendQueue;

		internal SimpleWebRTCClientWebGL(int maxMessageSize, int maxMessagesPerTick) : base(maxMessageSize, maxMessagesPerTick)
		{
#if !UNITY_WEBGL || UNITY_EDITOR
			throw new NotSupportedException();
#endif
		}

		public bool CheckJsConnected() => SimpleWebRTCJSLib.IsConnectedRTC(index);

		public override void Connect(List<Common.ICEServer> iceServers, Uri serverAddress)
		{
			string iceServersString = "";
			for (int m = 0; m < iceServers.Count; m++)
			{
				Common.ICEServer s = iceServers[m];

				if (s.url.Contains(";;") || s.url
				.Contains("__") || s.username.Contains(";;") || s.username
				.Contains("__") || s.credential.Contains(";;") || s.credential
				.Contains("__"))
				{
					throw new Exception("ICEServer params cannot have the strings ';;' or '__'");
				}

				iceServersString += s.url;
				if (s.username != "" && s.username != null)
					iceServersString += "__" + s.username + "__" + s.credential;
				if (m != iceServers.Count - 1)
					iceServersString += ";;";
			}

#if UNITY_WEBGL && !UNITY_EDITOR
			index = SimpleWebRTCJSLib.ConnectRTC(serverAddress.ToString(), iceServersString, OpenCallback, CloseCallBack, MessageCallback, ErrorCallback);
#endif
			instances.Add(index, this);
			state = ClientState.Connecting;
		}

		public override void Disconnect()
		{
			state = ClientState.Disconnecting;
			// disconnect should cause closeCallback and OnDisconnect to be called
			SimpleWebRTCJSLib.DisconnectRTC(index);
		}

		public override void Send(ArraySegment<byte> segment, Common.DeliveryMethod dm)
		{
			if (segment.Count > maxMessageSize)
			{
				Log.Error($"Cant send message with length {segment.Count} because it is over the max size of {maxMessageSize}");
				return;
			}

			if (state == ClientState.Connected)
			{
				SimpleWebRTCJSLib.SendRTC(index, segment.Array, segment.Offset, segment.Count, (int)dm);
			}
			else
			{
				if (ConnectingSendQueue == null)
					ConnectingSendQueue = new Queue<byte[]>();
				ConnectingSendQueue.Enqueue(segment.ToArray());
			}
		}

		void onOpen()
		{
			receiveQueue.Enqueue(new Message(Common.EventType.Connected));
			state = ClientState.Connected;

			if (ConnectingSendQueue != null)
			{
				while (ConnectingSendQueue.Count > 0)
				{
					byte[] next = ConnectingSendQueue.Dequeue();
					SimpleWebRTCJSLib.SendRTC(index, next, 0, next.Length, -1);
				}
				ConnectingSendQueue = null;
			}
		}

		void onClose()
		{
			// this code should be last in this class

			receiveQueue.Enqueue(new Message(Common.EventType.Disconnected));
			state = ClientState.NotConnected;
			instances.Remove(index);
		}

		void onMessage(IntPtr bufferPtr, int count)
		{
			try
			{
				ArrayBuffer buffer = bufferPool.Take(count);
				buffer.CopyFrom(bufferPtr, count);

				receiveQueue.Enqueue(new Message(buffer));
			}
			catch (Exception e)
			{
				Log.Error($"onData {e.GetType()}: {e.Message}\n{e.StackTrace}");
				receiveQueue.Enqueue(new Message(e));
			}
		}

		void onErr()
		{
			receiveQueue.Enqueue(new Message(new Exception("Javascript WebRTC error")));
			Disconnect();
		}

		[MonoPInvokeCallback(typeof(Action<int>))]
		static void OpenCallback(int index) => instances[index].onOpen();

		[MonoPInvokeCallback(typeof(Action<int>))]
		static void CloseCallBack(int index) => instances[index].onClose();

		[MonoPInvokeCallback(typeof(Action<int, IntPtr, int>))]
		static void MessageCallback(int index, IntPtr bufferPtr, int count) => instances[index].onMessage(bufferPtr, count);

		[MonoPInvokeCallback(typeof(Action<int>))]
		static void ErrorCallback(int index) => instances[index].onErr();
	}
}
