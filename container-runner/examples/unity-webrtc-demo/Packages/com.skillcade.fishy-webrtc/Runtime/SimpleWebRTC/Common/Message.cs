using System;

namespace cakeslice.SimpleWebRTC
{
	public struct Message
	{
		public readonly int connId;
		public readonly Common.EventType type;
		public readonly ArrayBuffer data;
		public readonly Exception exception;

		public Message(Common.EventType type) : this()
		{
			this.type = type;
		}

		public Message(ArrayBuffer data) : this()
		{
			type = Common.EventType.Data;
			this.data = data;
		}

		public Message(Exception exception) : this()
		{
			type = Common.EventType.Error;
			this.exception = exception;
		}

		public Message(int connId, Common.EventType type) : this()
		{
			this.connId = connId;
			this.type = type;
		}

		public Message(int connId, ArrayBuffer data) : this()
		{
			this.connId = connId;
			type = Common.EventType.Data;
			this.data = data;
		}

		public Message(int connId, Exception exception) : this()
		{
			this.connId = connId;
			type = Common.EventType.Error;
			this.exception = exception;
		}
	}
}
