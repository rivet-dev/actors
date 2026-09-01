using FishNet.Utility.Performance;
using LiteNetLib;
using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Threading.Tasks;

namespace FishNet.Transporting.Tugboat
{
    public abstract class CommonSocket
    {
        #region Internal.
        /// <summary>
        /// Current ConnectionState.
        /// </summary>
        private LocalConnectionState _connectionState = LocalConnectionState.Stopped;

        /// <summary>
        /// Returns the current ConnectionState.
        /// </summary>
        /// <returns></returns>
        internal LocalConnectionState GetConnectionState()
        {
            return _connectionState;
        }

        /// <summary>
        /// Sets a new connection state.
        /// </summary>
        /// <param name = "connectionState"></param>
        protected void SetConnectionState(LocalConnectionState connectionState, bool asServer)
        {
            // If state hasn't changed.
            if (connectionState == _connectionState)
                return;

            _connectionState = connectionState;
            if (asServer)
                Transport.HandleServerConnectionState(new(connectionState, Transport.Index));
            else
                Transport.HandleClientConnectionState(new(connectionState, Transport.Index));
        }
        #endregion

        #region Internal.
        /// <summary>
        /// NetManager for this socket.
        /// </summary>
        internal NetManager NetManager;
        #endregion

        #region Protected.
        /// <summary>
        /// Changes to the sockets local connection state.
        /// </summary>
        protected ConcurrentQueue<LocalConnectionState> LocalConnectionStates = new();
        /// <summary>
        /// Transport controlling this socket.
        /// </summary>
        protected Transport Transport;
        #endregion

        #region Private.
        /// <summary>
        /// Locks the NetManager to stop it.
        /// </summary>
        private readonly object _stopLock = new();
        #endregion

        /// <summary>
        /// Sends data to connectionId.
        /// </summary>
        internal void Send(ref Queue<Packet> queue, byte channelId, ArraySegment<byte> segment, int connectionId, int mtu)
        {
            if (GetConnectionState() != LocalConnectionState.Started)
                return;

            // ConnectionId isn't used from client to server.
            Packet outgoing = new(connectionId, segment, channelId, mtu);
            queue.Enqueue(outgoing);
        }

        /// <summary>
        /// Updates the timeout for NetManager.
        /// </summary>
        protected void UpdateTimeout(NetManager netManager, int timeout)
        {
            if (netManager == null)
                return;

            timeout = timeout == 0 ? int.MaxValue : Math.Min(int.MaxValue, timeout * 1000);
            netManager.DisconnectTimeout = timeout;
        }

        /// <summary>
        /// Clears a ConcurrentQueue of any type.
        /// </summary>
        internal void ClearGenericQueue<T>(ref ConcurrentQueue<T> queue)
        {
            while (queue.TryDequeue(out _)) { }
        }

        /// <summary>
        /// Clears a queue using Packet type.
        /// </summary>
        /// <param name = "queue"></param>
        internal void ClearPacketQueue(ref ConcurrentQueue<Packet> queue)
        {
            while (queue.TryDequeue(out Packet p))
                p.Dispose();
        }

        /// <summary>
        /// Clears a queue using Packet type.
        /// </summary>
        /// <param name = "queue"></param>
        internal void ClearPacketQueue(ref Queue<Packet> queue)
        {
            int count = queue.Count;
            for (int i = 0; i < count; i++)
            {
                Packet p = queue.Dequeue();
                p.Dispose();
            }
        }

        /// <summary>
        /// Called when data is received.
        /// </summary>
        internal virtual void Listener_NetworkReceiveEvent(ConcurrentQueue<Packet> queue, NetPeer fromPeer, NetPacketReader reader, DeliveryMethod deliveryMethod, int mtu)
        {
            // Set buffer.
            int dataLen = reader.AvailableBytes;
            // Prefer to max out returned array to mtu to reduce chance of resizing.
            int arraySize = Math.Max(dataLen, mtu);
            byte[] data = ByteArrayPool.Retrieve(arraySize);
            reader.GetBytes(data, dataLen);
            //Id.
            int id = fromPeer.Id;
            //Channel.
            byte channel = deliveryMethod == DeliveryMethod.Unreliable ? (byte)Channel.Unreliable : (byte)Channel.Reliable;
            //Add to packets.
            Packet packet = new(id, data, dataLen, channel);
            queue.Enqueue(packet);
            //Recycle reader.
            reader.Recycle();
        }

        internal void PollSocket(NetManager nm)
        {
            nm?.PollEvents();
        }

        /// <summary>
        /// Stops the socket and updates local connection state.
        /// </summary>
        protected void StopSocket()
        {
            if (NetManager == null)
                return;

            const bool threaded = false;
            // bool threaded;
            // if (Transport is Tugboat tb)
            //     threaded = tb.StopSocketsOnThread;
            // else
            //     threaded = false;

            //If using a thread.
            if (threaded)
            {
                Task.Run(() =>
                {
                    lock (_stopLock)
                    {
                        NetManager?.Stop();
                        NetManager = null;
                    }

                    //If not stopped yet also enqueue stop.
                    if (GetConnectionState() != LocalConnectionState.Stopped)
                        LocalConnectionStates.Enqueue(LocalConnectionState.Stopped);
                });
            }
            //Not using a thread.
            else
            {
                NetManager?.Stop();
                NetManager = null;
                //If not stopped yet also enqueue stop.
                if (GetConnectionState() != LocalConnectionState.Stopped)
                    LocalConnectionStates.Enqueue(LocalConnectionState.Stopped);
            }
        }

        /// <summary>
        /// Returns the port from the socket if active, otherwise returns null.
        /// </summary>
        /// <returns></returns>
        internal ushort? GetPort()
        {
            if (NetManager == null || !NetManager.IsRunning)
                return null;

            int port = NetManager.LocalPort;
            if (port < 0)
                port = 0;
            else if (port > ushort.MaxValue)
                port = ushort.MaxValue;

            return (ushort)port;
        }
    }
}