using FishNet.Connection;
using FishNet.Managing;
using LiteNetLib;
using LiteNetLib.Layers;
using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Net;

namespace FishNet.Transporting.Tugboat.Server
{
    public class ServerSocket : CommonSocket
    {
        #region Public.
        /// <summary>
        /// Gets the current ConnectionState of a remote client on the server.
        /// </summary>
        /// <param name = "connectionId">ConnectionId to get ConnectionState for.</param>
        internal RemoteConnectionState GetConnectionState(int connectionId)
        {
            NetPeer peer = GetNetPeer(connectionId, false);
            if (peer == null || peer.ConnectionState != ConnectionState.Connected)
                return RemoteConnectionState.Stopped;
            else
                return RemoteConnectionState.Started;
        }
        #endregion

        #region Private.
        #region Configuration.
        /// <summary>
        /// Port used by server.
        /// </summary>
        private ushort _port;
        /// <summary>
        /// Maximum number of allowed clients.
        /// </summary>
        private int _maximumClients;
        /// <summary>
        /// MTU size per packet.
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
        /// <summary>
        /// ConnectionEvents which need to be handled.
        /// </summary>
        private ConcurrentQueue<RemoteConnectionEvent> _remoteConnectionEvents = new();
        #endregion

        /// <summary>
        /// How long in seconds until client times from server.
        /// </summary>
        private int _timeout;
        /// <summary>
        /// IPv4 address to bind server to.
        /// </summary>
        private string _ipv4BindAddress;
        /// <summary>
        /// IPv6 address to bind server to.
        /// </summary>
        private string _ipv6BindAddress;
        /// <summary>
        /// PacketLayer to use with LiteNetLib.
        /// </summary>
        private PacketLayerBase _packetLayer;
        /// <summary>
        /// IPv6 is enabled only on demand, by default LiteNetLib always listens on IPv4 AND IPv6 which causes problems
        /// if IPv6 is disabled on host. This can be the case in Linux environments
        /// </summary>
        private bool _enableIPv6;
        #endregion

        ~ServerSocket()
        {
            StopConnection();
        }

        /// <summary>
        /// Initializes this for use.
        /// </summary>
        /// <param name = "t"></param>
        internal void Initialize(Transport t, int unreliableMTU, PacketLayerBase packetLayer, bool enableIPv6)
        {
            Transport = t;
            _mtu = unreliableMTU;
            _packetLayer = packetLayer;
            _enableIPv6 = enableIPv6;
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
        /// Threaded operation to process server actions.
        /// </summary>
        private void StartSocket()
        {
            EventBasedNetListener listener = new();
            listener.ConnectionRequestEvent += Listener_ConnectionRequestEvent;
            listener.PeerConnectedEvent += Listener_PeerConnectedEvent;
            listener.NetworkReceiveEvent += Listener_NetworkReceiveEvent;
            listener.PeerDisconnectedEvent += Listener_PeerDisconnectedEvent;

            NetManager = new(listener, _packetLayer, false);
            NetManager.DontRoute = ((Tugboat)Transport).DontRoute;
            NetManager.ReuseAddress = ((Tugboat)Transport).ReuseAddress;
            NetManager.MtuOverride = _mtu;// + NetConstants.FragmentedHeaderTotalSize;
            
            UpdateTimeout(_timeout);

            // Set bind addresses.
            IPAddress ipv4 = null;
            IPAddress ipv6 = null;

            // Set ipv4
            if (!string.IsNullOrEmpty(_ipv4BindAddress))
            {
                if (!IPAddress.TryParse(_ipv4BindAddress, out ipv4))
                    ipv4 = null;

                // If unable to parse try to get address another way.
                if (ipv4 == null)
                {
                    IPHostEntry hostEntry = Dns.GetHostEntry(_ipv4BindAddress);
                    if (hostEntry.AddressList.Length > 0)
                    {
                        ipv4 = hostEntry.AddressList[0];
                        Transport.NetworkManager.Log($"IPv4 could not parse correctly but was resolved to {ipv4.ToString()}");
                    }
                }
            }
            else
            {
                IPAddress.TryParse("0.0.0.0", out ipv4);
            }

            if (_enableIPv6 && !string.IsNullOrEmpty(_ipv6BindAddress))
            {
                // Set ipv6 if protocol is enabled.
                if (!IPAddress.TryParse(_ipv6BindAddress, out ipv6))
                    ipv6 = null;
            }
            else
            {
                IPAddress.TryParse("0:0:0:0:0:0:0:0", out ipv6);
            }

            string ipv4FailText = ipv4 == null ? $"IPv4 address {_ipv4BindAddress} failed to parse. " : string.Empty;
            string ipv6FailText = _enableIPv6 && ipv6 == null ? $"IPv6 address {_ipv6BindAddress} failed to parse. " : string.Empty;
            if (ipv4FailText != string.Empty || ipv6FailText != string.Empty)
            {
                Transport.NetworkManager.Log($"{ipv4FailText}{ipv6FailText}Clear the bind address field to use any bind address.");
                StopConnection();
                return;
            }

            NetManager.IPv6Enabled = _enableIPv6;
            bool startResult = NetManager.Start(ipv4, ipv6, _port);
            //If started succcessfully.
            if (startResult)
            {
                LocalConnectionStates.Enqueue(LocalConnectionState.Started);
            }
            //Failed to start.
            else
            {
                Transport.NetworkManager.LogError($"Server failed to start. This usually occurs when the specified port is unavailable, be it closed or already in use.");
                StopConnection();
            }
        }

        /// <summary>
        /// Gets the address of a remote connection Id.
        /// </summary>
        /// <param name = "connectionId"></param>
        /// <returns>Returns string.empty if Id is not found.</returns>
        internal string GetConnectionAddress(int connectionId)
        {
            if (GetConnectionState() != LocalConnectionState.Started)
            {
                NetworkManager nm = Transport == null ? null : Transport.NetworkManager;
                string msg = "Server socket is not started.";
                nm.LogWarning(msg);
                return string.Empty;
            }

            NetPeer peer = GetNetPeer(connectionId, false);
            if (peer == null)
            {
                Transport.NetworkManager.LogWarning($"Connection Id {connectionId} returned a null NetPeer.");
                return string.Empty;
            }

            return peer.Address.ToString();
        }

        /// <summary>
        /// Returns a NetPeer for connectionId.
        /// </summary>
        /// <param name = "connectionId"></param>
        /// <returns></returns>
        private NetPeer GetNetPeer(int connectionId, bool connectedOnly)
        {
            if (NetManager != null)
            {
                NetPeer peer = NetManager.GetPeerById(connectionId);
                if (connectedOnly && peer != null && peer.ConnectionState != ConnectionState.Connected)
                    peer = null;

                return peer;
            }
            else
            {
                return null;
            }
        }

        /// <summary>
        /// Starts the server.
        /// </summary>
        internal bool StartConnection(ushort port, int maximumClients, string ipv4BindAddress, string ipv6BindAddress)
        {
            //Force a stop just in case the socket did not clean up.
            if (base.GetConnectionState() != LocalConnectionState.Stopped)
                StopSocket();
            //Enqueue starting.
            LocalConnectionStates.Enqueue(LocalConnectionState.Starting);
            //Iterate to cause state changes to invoke.
            IterateIncoming();

            //Assign properties.
            _port = port;
            _maximumClients = maximumClients;
            _ipv4BindAddress = ipv4BindAddress;
            _ipv6BindAddress = ipv6BindAddress;
            ResetQueues();

            StartSocket();
            //Task.Run(StartSocket);

            return true;
        }

        /// <summary>
        /// Stops the local socket.
        /// </summary>
        internal bool StopConnection()
        {
            if (NetManager == null || base.GetConnectionState() == LocalConnectionState.Stopped || base.GetConnectionState() == LocalConnectionState.Stopping)
                return false;

            LocalConnectionStates.Enqueue(LocalConnectionState.Stopping);
            StopSocket();
            return true;
        }

        /// <summary>
        /// Stops a remote client disconnecting the client from the server.
        /// </summary>
        /// <param name = "connectionId">ConnectionId of the client to disconnect.</param>
        internal bool StopConnection(int connectionId)
        {
            //Server isn't running.
            if (NetManager == null || base.GetConnectionState() != LocalConnectionState.Started)
                return false;

            NetPeer peer = GetNetPeer(connectionId, false);
            if (peer == null)
                return false;

            try
            {
                peer.Disconnect();
                //Let LiteNetLib get the disconnect event which will enqueue a remote connection state.
                //base.Transport.HandleRemoteConnectionState(new RemoteConnectionStateArgs(RemoteConnectionState.Stopped, connectionId, base.Transport.Index));
            }
            catch
            {
                return false;
            }

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
            ClearGenericQueue(ref _remoteConnectionEvents);
        }

        /// <summary>
        /// Called when a peer disconnects or times out.
        /// </summary>
        private void Listener_PeerDisconnectedEvent(NetPeer peer, DisconnectInfo disconnectInfo)
        {
            _remoteConnectionEvents.Enqueue(new(false, peer.Id));
        }

        /// <summary>
        /// Called when a peer completes connection.
        /// </summary>
        private void Listener_PeerConnectedEvent(NetPeer peer)
        {
            _remoteConnectionEvents.Enqueue(new(true, peer.Id));
        }

        /// <summary>
        /// Called when data is received from a peer.
        /// </summary>
        private void Listener_NetworkReceiveEvent(NetPeer fromPeer, NetPacketReader reader, byte channel, DeliveryMethod deliveryMethod)
        {
            //If over the MTU.
            if (reader.AvailableBytes > _mtu)
            {
                _remoteConnectionEvents.Enqueue(new(false, fromPeer.Id));
                fromPeer.Disconnect();
            }
            else
            {
                base.Listener_NetworkReceiveEvent(_incoming, fromPeer, reader, deliveryMethod, _mtu);
            }
        }

        /// <summary>
        /// Called when a remote connection request is made.
        /// </summary>
        private void Listener_ConnectionRequestEvent(ConnectionRequest request)
        {
            if (NetManager == null)
                return;

            //At maximum peers.
            if (NetManager.ConnectedPeersCount >= _maximumClients)
            {
                request.Reject();
                return;
            }

            request.AcceptIfKey(key: string.Empty);
        }

        /// <summary>
        /// Dequeues and processes outgoing.
        /// </summary>
        private void DequeueOutgoing()
        {
            if (base.GetConnectionState() != LocalConnectionState.Started || NetManager == null)
            {
                //Not started, clear outgoing.
                ClearPacketQueue(ref _outgoing);
            }
            else
            {
                int count = _outgoing.Count;
                for (int i = 0; i < count; i++)
                {
                    Packet outgoing = _outgoing.Dequeue();
                    int connectionId = outgoing.ConnectionId;

                    ArraySegment<byte> segment = outgoing.GetArraySegment();
                    DeliveryMethod dm = outgoing.Channel == (byte)Channel.Reliable ? DeliveryMethod.ReliableOrdered : DeliveryMethod.Unreliable;

                    //If over the MTU.
                    if (outgoing.Channel == (byte)Channel.Unreliable && segment.Count > _mtu)
                    {
                        Transport.NetworkManager.LogWarning($"Server is sending of {segment.Count} length on the unreliable channel, while the MTU is only {_mtu}. The channel has been changed to reliable for this send.");
                        dm = DeliveryMethod.ReliableOrdered;
                    }

                    //Send to all clients.
                    if (connectionId == NetworkConnection.UNSET_CLIENTID_VALUE)
                    {
                        NetManager.SendToAll(segment.Array, segment.Offset, segment.Count, dm);
                    }
                    //Send to one client.
                    else
                    {
                        NetPeer peer = GetNetPeer(connectionId, true);
                        //If peer is found.
                        if (peer != null)
                            peer.Send(segment.Array, segment.Offset, segment.Count, dm);
                    }

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
                SetConnectionState(result, true);

            //Not yet started.
            LocalConnectionState localState = base.GetConnectionState();
            if (localState != LocalConnectionState.Started)
            {
                ResetQueues();
                //If stopped try to kill task.
                if (localState == LocalConnectionState.Stopped)
                {
                    StopSocket();
                    return;
                }
            }

            //Handle connection and disconnection events.
            while (_remoteConnectionEvents.TryDequeue(out RemoteConnectionEvent connectionEvent))
            {
                RemoteConnectionState state = connectionEvent.Connected ? RemoteConnectionState.Started : RemoteConnectionState.Stopped;
                Transport.HandleRemoteConnectionState(new(state, connectionEvent.ConnectionId, Transport.Index));
            }

            //Handle packets.
            while (_incoming.TryDequeue(out Packet incoming))
            {
                //Make sure peer is still connected.
                NetPeer peer = GetNetPeer(incoming.ConnectionId, true);
                if (peer != null)
                {
                    ServerReceivedDataArgs dataArgs = new(incoming.GetArraySegment(), (Channel)incoming.Channel, incoming.ConnectionId, Transport.Index);

                    Transport.HandleServerReceivedDataArgs(dataArgs);
                }

                incoming.Dispose();
            }
        }

        /// <summary>
        /// Sends a packet to a single, or all clients.
        /// </summary>
        internal void SendToClient(byte channelId, ArraySegment<byte> segment, int connectionId)
        {
            Send(ref _outgoing, channelId, segment, connectionId, _mtu);
        }

        /// <summary>
        /// Returns the maximum number of clients allowed to connect to the server. If the transport does not support this method the value -1 is returned.
        /// </summary>
        /// <returns></returns>
        internal int GetMaximumClients()
        {
            return Math.Min(_maximumClients, NetworkConnection.MAXIMUM_CLIENTID_WITHOUT_SIMULATED_VALUE);
        }

        /// <summary>
        /// Sets the MaximumClients value.
        /// </summary>
        /// <param name = "value"></param>
        internal void SetMaximumClients(int value)
        {
            _maximumClients = value;
        }
    }
}