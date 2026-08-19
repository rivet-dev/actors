using FishNet.Managing;
using LiteNetLib;
using LiteNetLib.Layers;
using System;
using System.Collections.Concurrent;
using System.Collections.Generic;

namespace FishNet.Transporting.Tugboat.Client
{
    public class ClientSocket : CommonSocket
    {
        ~ClientSocket()
        {
            StopConnection();
        }

        #region Private.
        #region Configuration.
        /// <summary>
        /// Address to bind server to.
        /// </summary>
        private string _address = string.Empty;
        /// <summary>
        /// Port used by server.
        /// </summary>
        private ushort _port;
        /// <summary>
        /// MTU sizes for each channel.
        /// </summary>
        private int _mtu;
        #endregion

        #region Queues.
        /// <summary>
        /// Inbound messages which need to be handled.
        /// </summary>
        private ConcurrentQueue<Packet> _incoming = new();
        /// <summary>
        /// Outbound messages which need to be handled.
        /// </summary>
        private Queue<Packet> _outgoing = new();
        #endregion

        /// <summary>
        /// How long in seconds until client times from server.
        /// </summary>
        private int _timeout;
        /// <summary>
        /// PacketLayer to use with LiteNetLib.
        /// </summary>
        private PacketLayerBase _packetLayer;
        #endregion

        /// <summary>
        /// Initializes this for use.
        /// </summary>
        internal void Initialize(Transport t, int unreliableMTU, PacketLayerBase packetLayer)
        {
            Transport = t;
            _mtu = unreliableMTU;
            _packetLayer = packetLayer;
        }

        /// <summary>
        /// Updates the Timeout value as seconds.
        /// </summary>
        internal void UpdateTimeout(int timeout)
        {
            _timeout = timeout;
            base.UpdateTimeout(NetManager, timeout);
        }

        /// <summary>
        /// Polls the socket for new data.
        /// </summary>
        internal void PollSocket()
        {
            base.PollSocket(NetManager);
        }

        /// <summary>
        /// Threaded operation to process client actions.
        /// </summary>
        private void StartSocket()
        {
            EventBasedNetListener listener = new();
            listener.NetworkReceiveEvent += Listener_NetworkReceiveEvent;
            listener.PeerConnectedEvent += Listener_PeerConnectedEvent;
            listener.PeerDisconnectedEvent += Listener_PeerDisconnectedEvent;

            NetManager = new(listener, _packetLayer, false);
            NetManager.DontRoute = ((Tugboat)Transport).DontRoute;
            NetManager.MtuOverride = _mtu;// + NetConstants.FragmentedHeaderTotalSize;

            UpdateTimeout(_timeout);

            LocalConnectionStates.Enqueue(LocalConnectionState.Starting);
            NetManager.Start();
            NetManager.Connect(_address, _port, string.Empty);
        }

        /// <summary>
        /// Starts the client connection.
        /// </summary>
        internal bool StartConnection(string address, ushort port)
        {
            // Force a stop just in case the socket did not clean up.
            if (GetConnectionState() != LocalConnectionState.Stopped)
                StopSocket();
            // Enqueue starting.
            LocalConnectionStates.Enqueue(LocalConnectionState.Starting);
            // Iterate to cause state changes to invoke.
            IterateIncoming();

            // Assign properties.
            _port = port;
            _address = address;

            ResetQueues();
            
            StartSocket();
            //Task.Run(StartSocket);

            return true;
        }

        /// <summary>
        /// Stops the local socket.
        /// </summary>
        internal bool StopConnection(DisconnectInfo? info = null)
        {
            if (GetConnectionState() == LocalConnectionState.Stopped || GetConnectionState() == LocalConnectionState.Stopping)
                return false;

            if (info != null)
                Transport.NetworkManager.Log($"Local client disconnect reason: {info.Value.Reason}.");

            SetConnectionState(LocalConnectionState.Stopping, false);
            StopSocket();
            return true;
        }

        /// <summary>
        /// Resets queues.
        /// </summary>
        private void ResetQueues()
        {
            ClearGenericQueue(ref LocalConnectionStates);
            ClearPacketQueue(ref _incoming);
            ClearPacketQueue(ref _outgoing);
        }

        /// <summary>
        /// Called when disconnected from the server.
        /// </summary>
        private void Listener_PeerDisconnectedEvent(NetPeer peer, DisconnectInfo disconnectInfo)
        {
            StopConnection(disconnectInfo);
        }

        /// <summary>
        /// Called when connected to the server.
        /// </summary>
        private void Listener_PeerConnectedEvent(NetPeer peer)
        {
            LocalConnectionStates.Enqueue(LocalConnectionState.Started);
        }

        /// <summary>
        /// Called when data is received from a peer.
        /// </summary>
        private void Listener_NetworkReceiveEvent(NetPeer fromPeer, NetPacketReader reader, byte channel, DeliveryMethod deliveryMethod)
        {
            base.Listener_NetworkReceiveEvent(_incoming, fromPeer, reader, deliveryMethod, _mtu);
        }

        /// <summary>
        /// Dequeues and processes outgoing.
        /// </summary>
        private void DequeueOutgoing()
        {
            NetPeer peer = null;
            if (NetManager != null)
                peer = NetManager.FirstPeer;
            // Server connection hasn't been made.
            if (peer == null)
            {
                /* Only dequeue outgoing because other queues might have
                 * relevant information, such as the local connection queue. */
                ClearPacketQueue(ref _outgoing);
            }
            else
            {
                int count = _outgoing.Count;
                for (int i = 0; i < count; i++)
                {
                    Packet outgoing = _outgoing.Dequeue();

                    ArraySegment<byte> segment = outgoing.GetArraySegment();
                    DeliveryMethod dm = outgoing.Channel == (byte)Channel.Reliable ? DeliveryMethod.ReliableOrdered : DeliveryMethod.Unreliable;

                    // If over the MTU.
                    if (outgoing.Channel == (byte)Channel.Unreliable && segment.Count > _mtu)
                    {
                        Transport.NetworkManager.LogWarning($"Client is sending of {segment.Count} length on the unreliable channel, while the MTU is only {_mtu}. The channel has been changed to reliable for this send.");
                        dm = DeliveryMethod.ReliableOrdered;
                    }

                    peer.Send(segment.Array, segment.Offset, segment.Count, dm);

                    outgoing.Dispose();
                }
            }
        }

        /// <summary>
        /// Allows for Outgoing queue to be iterated.
        /// </summary>
        internal void IterateOutgoing()
        {
            DequeueOutgoing();
        }

        /// <summary>
        /// Iterates the Incoming queue.
        /// </summary>
        internal void IterateIncoming()
        {
            /* Run local connection states first so we can begin
             * to read for data at the start of the frame, as that's
             * where incoming is read. */
            while (LocalConnectionStates.TryDequeue(out LocalConnectionState result))
                SetConnectionState(result, false);

            // Not yet started, cannot continue.
            LocalConnectionState localState = GetConnectionState();
            if (localState != LocalConnectionState.Started)
            {
                ResetQueues();
                // If stopped try to kill task.
                if (localState == LocalConnectionState.Stopped)
                {
                    StopSocket();
                    return;
                }
            }

            /* Incoming. */
            while (_incoming.TryDequeue(out Packet incoming))
            {
                ClientReceivedDataArgs dataArgs = new(incoming.GetArraySegment(), (Channel)incoming.Channel, Transport.Index);
                Transport.HandleClientReceivedDataArgs(dataArgs);
                // Dispose of packet.
                incoming.Dispose();
            }
        }

        /// <summary>
        /// Sends a packet to the server.
        /// </summary>
        internal void SendToServer(byte channelId, ArraySegment<byte> segment)
        {
            // Not started, cannot send.
            if (GetConnectionState() != LocalConnectionState.Started)
                return;

            Send(ref _outgoing, channelId, segment, -1, _mtu);
        }
    }
}