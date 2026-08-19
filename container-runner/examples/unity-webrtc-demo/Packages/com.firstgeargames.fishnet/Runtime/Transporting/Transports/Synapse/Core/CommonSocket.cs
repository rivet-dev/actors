using System.Collections.Concurrent;
using System.Collections.Generic;
using System;

namespace FishNet.Transporting.Synapse
{

    /// <summary>
    /// Abstract base for client and server sockets.
    /// Manages local connection state and packet queuing.
    /// </summary>
    public abstract class CommonSocket
    {
        /// <summary>
        /// Current local connection state.
        /// </summary>
        private LocalConnectionState _connectionState = LocalConnectionState.Stopped;

        /// <summary>
        /// Pending local connection state changes to be applied on the main thread.
        /// </summary>
        protected ConcurrentQueue<LocalConnectionState> LocalConnectionStates = new();

        /// <summary>
        /// Transport that owns this socket.
        /// </summary>
        protected Transport Transport;

        /// <summary>
        /// Returns the current local connection state.
        /// </summary>
        internal LocalConnectionState GetConnectionState() => _connectionState;

        /// <summary>
        /// Applies a new local connection state and notifies the transport.
        /// </summary>
        protected void SetConnectionState(LocalConnectionState connectionState, bool isServer)
        {
            if (connectionState == _connectionState)
                return;

            _connectionState = connectionState;

            if (isServer)
                Transport.HandleServerConnectionState(new(connectionState, Transport.Index));
            else
                Transport.HandleClientConnectionState(new(connectionState, Transport.Index));
        }

        /// <summary>
        /// Enqueues an outgoing packet to the provided queue.
        /// Does nothing if the socket is not started.
        /// </summary>
        internal void Send(ref Queue<Packet> queue, byte channelId, ArraySegment<byte> segment, int connectionId)
        {
            if (GetConnectionState() != LocalConnectionState.Started)
                return;

            Packet outgoing = new(connectionId, segment, channelId);
            queue.Enqueue(outgoing);
        }

        /// <summary>
        /// Drains all entries from a concurrent queue of any type.
        /// </summary>
        internal void ClearGenericQueue<T>(ref ConcurrentQueue<T> queue)
        {
            while (queue.TryDequeue(out _)) { }
        }

        /// <summary>
        /// Drains and disposes all packets from a concurrent packet queue.
        /// </summary>
        internal void ClearPacketQueue(ref ConcurrentQueue<Packet> queue)
        {
            while (queue.TryDequeue(out Packet packet))
                packet.Dispose();
        }

        /// <summary>
        /// Drains and disposes all packets from a packet queue.
        /// </summary>
        internal void ClearPacketQueue(ref Queue<Packet> queue)
        {
            int count = queue.Count;

            for (int i = 0; i < count; i++)
            {
                Packet packet = queue.Dequeue();
                packet.Dispose();
            }
        }
    }
}
